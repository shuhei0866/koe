use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use super::{
    api::{read_json, transport, Api, TokenResponse},
    Outcome, Problem,
};

const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const CALLBACK: &str = "/auth/callback";
const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const BROWSER_WAIT: Duration = Duration::from_secs(180);

pub(super) struct Registration {
    pub client_id: String,
    pub email: Option<String>,
    pub id_token: Option<String>,
    pub consent: bool,
}

pub(super) struct Authorization {
    listener: TcpListener,
    redirect: String,
    state: String,
    nonce: String,
    verifier: String,
    saved_id: Option<String>,
    pub url: Url,
}

fn random() -> Outcome<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Problem::InvalidSignIn)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

impl Authorization {
    pub async fn start(api: &Api, host: &str, account: Option<&Registration>) -> Outcome<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|_| Problem::Browser)?;
        let port = listener.local_addr().map_err(|_| Problem::Browser)?.port();
        let redirect = format!("http://127.0.0.1:{port}{CALLBACK}");
        let state = random()?;
        let nonce = random()?;
        let verifier = random()?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = Url::parse(&format!("{}/api/accounts/authorize", api.issuer))
            .map_err(|_| Problem::InvalidSignIn)?;
        {
            let mut query = url.query_pairs_mut();
            query.extend_pairs([
                (
                    "client_id",
                    account.map_or(DYNAMIC_CLIENT, |a| &a.client_id),
                ),
                ("ext_agent_host_id", host),
                ("response_type", "code"),
                ("redirect_uri", &redirect),
                ("scope", SCOPE),
                ("resource", &api.resource),
                ("state", &state),
                ("nonce", &nonce),
                ("code_challenge_method", "S256"),
                ("code_challenge", &challenge),
            ]);
            if let Some(account) = account {
                if let Some(hint) = &account.email {
                    query.append_pair("login_hint", hint);
                }
                if let Some(hint) = &account.id_token {
                    query.append_pair("id_token_hint", hint);
                }
                if account.consent {
                    query.append_pair("prompt", "consent");
                }
            } else {
                query.append_pair("agent_name_hint", "Koe");
            }
        }
        Ok(Self {
            listener,
            redirect,
            state,
            nonce,
            verifier,
            url,
            saved_id: account.map(|a| a.client_id.clone()),
        })
    }

    pub async fn wait(&self) -> Outcome<(String, String, TcpStream)> {
        tokio::time::timeout(BROWSER_WAIT, async {
            loop {
                let (mut socket, _) = self.listener.accept().await.map_err(|_| Problem::Browser)?;
                let result =
                    tokio::time::timeout(Duration::from_secs(2), self.callback(&mut socket)).await;
                match result {
                    Ok(Some(Ok((client_id, code)))) => return Ok((client_id, code, socket)),
                    Ok(Some(Err(error))) => {
                        finish(socket, false).await;
                        return Err(error);
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| Problem::Timeout)?
    }

    async fn callback(&self, socket: &mut TcpStream) -> Option<Outcome<(String, String)>> {
        use tokio::io::AsyncReadExt;
        let mut request = Vec::new();
        let mut input = BufReader::new(socket).take(16 * 1024);
        input.read_until(b'\n', &mut request).await.ok()?;
        let mut header = Vec::new();
        loop {
            header.clear();
            if input.read_until(b'\n', &mut header).await.ok()? == 0 {
                return None;
            }
            if header == b"\r\n" {
                break;
            }
        }
        let line = std::str::from_utf8(&request).ok()?.strip_suffix("\r\n")?;
        let mut words = line.split_ascii_whitespace();
        if words.next()? != "GET" {
            return None;
        }
        let path = words.next()?.strip_prefix(CALLBACK)?.strip_prefix('?')?;
        let pairs: Vec<_> = url::form_urlencoded::parse(path.as_bytes()).collect();
        let unique = |name: &str| {
            let mut matches = pairs.iter().filter(|(key, _)| key == name);
            let value = matches.next()?.1.as_ref();
            matches.next().is_none().then_some(value)
        };
        if !bool::from(unique("state")?.as_bytes().ct_eq(self.state.as_bytes())) {
            return None;
        }
        if pairs.iter().any(|(key, _)| key == "error") {
            return Some(Err(if unique("error") == Some("access_denied") {
                Problem::ConsentDenied
            } else {
                Problem::InvalidSignIn
            }));
        }
        let supplied = unique("client_id");
        if supplied.is_none() && pairs.iter().any(|(key, _)| key == "client_id") {
            return Some(Err(Problem::InvalidSignIn));
        }
        let client_id = match (&self.saved_id, supplied) {
            (Some(saved), None) => saved,
            (Some(saved), Some(actual)) if saved == actual => saved,
            (None, Some(id)) if valid_client(id) => id,
            _ => return Some(Err(Problem::InvalidSignIn)),
        };
        let Some(code) = unique("code").filter(|s| !s.is_empty()) else {
            return Some(Err(Problem::InvalidSignIn));
        };
        Some(Ok((client_id.to_owned(), code.to_owned())))
    }

    pub async fn exchange(&self, api: &Api, client_id: &str, code: &str) -> Outcome<TokenResponse> {
        api.token(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", &self.verifier),
            ("redirect_uri", &self.redirect),
            ("resource", &api.resource),
        ])
        .await
    }

    pub async fn identity(&self, api: &Api, client_id: &str, token: &str) -> Outcome<Identity> {
        let header = decode_header(token).map_err(|_| Problem::InvalidSignIn)?;
        let kid = header.kid.ok_or(Problem::InvalidSignIn)?;
        if header.alg != Algorithm::RS256 {
            return Err(Problem::InvalidSignIn);
        }
        let discovery = api.discovery().await?;
        let jwks: JwkSet = read_json(
            api.http
                .get(discovery.jwks_uri)
                .send()
                .await
                .map_err(transport)?,
        )
        .await?;
        let jwk = jwks.find(&kid).ok_or(Problem::InvalidSignIn)?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| Problem::InvalidSignIn)?;
        verify(token, &key, &api.issuer, client_id, &self.nonce)
    }
}

fn valid_client(id: &str) -> bool {
    !id.is_empty()
        && id != DYNAMIC_CLIENT
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn verify(
    token: &str,
    key: &DecodingKey,
    issuer: &str,
    client_id: &str,
    nonce: &str,
) -> Outcome<Identity> {
    let mut checks = Validation::new(Algorithm::RS256);
    checks.set_issuer(&[issuer]);
    checks.set_audience(&[client_id]);
    checks.set_required_spec_claims(&["iss", "aud", "exp", "sub"]);
    checks.leeway = 30;
    let identity = decode::<Identity>(token, key, &checks)
        .map_err(|_| Problem::InvalidSignIn)?
        .claims;
    if identity.subject.is_empty()
        || !bool::from(identity.nonce.as_bytes().ct_eq(nonce.as_bytes()))
        || (matches!(&identity.aud, serde_json::Value::Array(ids) if ids.len() > 1)
            && identity.azp.is_none())
        || identity
            .azp
            .as_deref()
            .is_some_and(|party| party != client_id)
    {
        return Err(Problem::InvalidSignIn);
    }
    Ok(identity)
}

#[derive(Deserialize)]
pub(super) struct Identity {
    #[serde(rename = "sub")]
    pub subject: String,
    pub email: Option<String>,
    nonce: String,
    aud: serde_json::Value,
    azp: Option<String>,
}

pub(super) async fn finish(mut socket: TcpStream, success: bool) {
    let message = if success {
        "You can close this tab and return to Koe."
    } else {
        "ChatGPT sign-in did not complete. Return to Koe to try again."
    };
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{message}", message.len());
    let _ = tokio::time::timeout(
        Duration::from_millis(500),
        socket.write_all(response.as_bytes()),
    )
    .await;
}
