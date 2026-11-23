use std::time::Duration;

use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

use super::{
    api::Api,
    now,
    oauth::Authorization,
    session::{Account, Store, Tokens},
    ChatGpt, ChatGptProcessor, Problem,
};
use crate::{context::WindowContext, daemon::format_transcript, dictionary::Dictionary};

const RAW: &str = "えっとコエで東京タワーへ行く";
const CORRECTED: &str = "えっとKoeで東京タワーへ行く";
const FORMATTED: &str = "Koeで東京タワーへ行く。";
const OK_EVENTS: &str = concat!(
    "event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Koeで東京タワーへ行く。\"}\r\n\r\n",
    "event: response.completed\r\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\r\n\r\n"
);
const DELTA_ONLY: &str =
    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"途中までの結果\"}\n\n";
const USAGE_ERROR: &str =
    "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}}\n\n";

struct Reply {
    status: u16,
    content_type: Option<&'static str>,
    body: String,
    extra_bytes: usize,
    delay: Duration,
    split_at: Option<usize>,
}

impl Reply {
    fn event(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            content_type: Some("text/event-stream"),
            body: body.into(),
            extra_bytes: 0,
            delay: Duration::ZERO,
            split_at: None,
        }
    }

    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            content_type: Some("application/json"),
            ..Self::event(body.to_string())
        }
    }
}

#[derive(Debug)]
struct Request {
    line: String,
    headers: String,
    body: Vec<u8>,
}

struct Server(TcpListener);

impl Server {
    async fn start() -> Self {
        Self(TcpListener::bind(("127.0.0.1", 0)).await.unwrap())
    }

    fn url(&self) -> String {
        format!("http://{}", self.0.local_addr().unwrap())
    }

    fn api(&self) -> Api {
        Api {
            issuer: self.url(),
            resource: format!("{}/v1", self.url()),
            ..Api::new().unwrap()
        }
    }

    fn serve(self, replies: Vec<Reply>) -> JoinHandle<Vec<Request>> {
        tokio::spawn(async move {
            let mut requests = Vec::new();
            for reply in replies {
                let (stream, _) = self.0.accept().await.unwrap();
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                let mut headers = String::new();
                let length = loop {
                    let mut header = String::new();
                    assert_ne!(
                        stream.read_line(&mut header).await.unwrap(),
                        0,
                        "EOF in fixture headers"
                    );
                    if header == "\r\n" {
                        let length = headers.lines().find_map(|h| {
                            h.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        });
                        break length.unwrap_or_default();
                    }
                    headers.push_str(&header);
                };
                let mut body = vec![0; length];
                stream.read_exact(&mut body).await.unwrap();
                requests.push(Request {
                    line,
                    headers,
                    body,
                });
                let mut stream = stream.into_inner();
                let content_type = reply
                    .content_type
                    .map(|value| format!("Content-Type: {value}\r\n"))
                    .unwrap_or_default();
                let response = format!(
                    "HTTP/1.1 {} Fixture\r\nConnection: close\r\n{content_type}Content-Length: {}\r\n\r\n",
                    reply.status, reply.body.len() + reply.extra_bytes,
                );
                if stream.write_all(response.as_bytes()).await.is_ok() {
                    // Split Japanese across byte boundaries to exercise SSE framing.
                    let offset = reply
                        .split_at
                        .unwrap_or_else(|| reply.body.find('で').map_or(0, |i| i + 1));
                    let (head, tail) = reply.body.as_bytes().split_at(offset);
                    let _ = stream.write_all(head).await;
                    tokio::time::sleep(reply.delay).await;
                    let _ = stream.write_all(tail).await;
                }
            }
            requests
        })
    }
}

fn dictionary() -> Dictionary {
    let mut dictionary = Dictionary::default();
    dictionary.terms.insert("コエ".into(), "Koe".into());
    dictionary
        .terms
        .insert("秘密辞書の誤記".into(), "秘密辞書の語".into());
    dictionary.notes.push("秘密辞書メモ".into());
    dictionary
}

async fn utterance(processor: &ChatGptProcessor) -> crate::daemon::FinalTranscript {
    format_transcript(
        RAW,
        processor,
        &WindowContext {
            window_title: "送ってはいけないウィンドウ".into(),
            app_name: "送ってはいけないアプリ名".into(),
            ..Default::default()
        },
        &dictionary(),
        "送ってはいけない学習メモ",
        Ok(()),
    )
    .await
}

async fn format(api: &Api, max_bytes: usize) -> Result<String, Problem> {
    let started = std::time::Instant::now();
    let response = api.start_format("token", "model", CORRECTED).await?;
    Api::finish_format(response, max_bytes, started).await
}

#[tokio::test]
async fn model_catalog_uses_server_order_and_list_visibility() {
    let server = Server::start().await;
    let api = server.api();
    let requests = server.serve(vec![Reply::json(
        200,
        json!({"models": [
            {"slug": "preferred", "display_name": "Preferred", "visibility": "list"},
            {"slug": "hidden", "display_name": "Hidden", "visibility": "hide"},
            {"slug": "also-listed", "display_name": "Also listed", "visibility": "list"}
        ]}),
    )]);
    let models = api.models("synthetic-access").await.unwrap();
    assert_eq!(
        models.iter().map(|m| m.slug.as_str()).collect::<Vec<_>>(),
        ["preferred", "also-listed"]
    );
    assert_eq!(models[0].display_name, "Preferred");
    let request = requests.await.unwrap().pop().unwrap();
    assert_eq!(request.line, "GET /v1/models HTTP/1.1\r\n");
    assert!(request
        .headers
        .contains("authorization: Bearer synthetic-access"));
}

#[tokio::test]
async fn streaming_uses_completed_message_and_ignores_keepalives() {
    let server = Server::start().await;
    let api = server.api();
    let encode = |response: Value| {
        format!(
            "data: {}\r\n\r\n",
            json!({"type": "response.completed", "response": response})
        )
    };
    let message = json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": FORMATTED}]
    });
    let completed = json!({"status": "completed", "output": [message.clone()]});
    let requests = server.serve(vec![
        Reply::event(
            ": keepalive\r\n\r\nevent: heartbeat\r\n\r\nevent: ping\r\ndata: keep-alive\r\n\r\n"
                .to_owned()
                + &encode(completed),
        ),
        Reply::event(
            DELTA_ONLY.to_owned() + &encode(json!({"status": "incomplete", "output": [message]})),
        ),
        Reply::event(
            DELTA_ONLY.to_owned()
                + &encode(json!({"status": "completed", "output": [{
                    "type": "message", "role": "assistant",
                    "content": [{"type": "refusal", "refusal": "fixture"}]
                }]})),
        ),
        Reply::event("event: ping\ndata: keep-alive\n\n".to_owned() + DELTA_ONLY),
        Reply::event(DELTA_ONLY.to_owned() + &encode(json!({"status": "completed", "output": []}))),
    ]);
    assert_eq!(format(&api, 4096).await.unwrap(), FORMATTED);
    assert_eq!(format(&api, 4096).await, Err(Problem::Failed));
    assert_eq!(format(&api, 4096).await, Err(Problem::Failed));
    assert_eq!(format(&api, 4096).await, Err(Problem::Interrupted));
    assert_eq!(format(&api, 4096).await.unwrap(), "途中までの結果");
    assert_eq!(requests.await.unwrap().len(), 5);
}

#[tokio::test]
async fn streaming_without_content_type_still_requires_a_terminal_event() {
    let server = Server::start().await;
    let api = server.api();
    let requests = server.serve(vec![
        Reply {
            content_type: None,
            ..Reply::event(OK_EVENTS)
        },
        Reply {
            content_type: None,
            ..Reply::event(DELTA_ONLY)
        },
        Reply {
            content_type: None,
            ..Reply::json(200, json!({"status":"completed","output":FORMATTED}))
        },
        Reply {
            content_type: None,
            ..Reply::json(204, Value::Null)
        },
    ]);
    assert_eq!(format(&api, 4096).await.unwrap(), FORMATTED);
    for _ in 0..3 {
        assert_eq!(format(&api, 4096).await, Err(Problem::Interrupted));
    }
    assert_eq!(requests.await.unwrap().len(), 4);
}

#[tokio::test]
async fn streaming_with_a_split_bom_still_requires_a_real_response() {
    let server = Server::start().await;
    let api = server.api();
    let requests = server.serve(vec![
        Reply {
            delay: Duration::from_millis(20),
            split_at: Some(1),
            ..Reply::event("\u{feff}".to_owned() + OK_EVENTS)
        },
        Reply::event("\u{feff}\u{feff}".to_owned() + OK_EVENTS),
        Reply::event(DELTA_ONLY.to_owned() + "data: {\"type\":\"response.completed\"}\n\n"),
    ]);
    assert_eq!(format(&api, 4096).await.unwrap(), FORMATTED);
    assert_eq!(format(&api, 4096).await.unwrap(), FORMATTED);
    assert_eq!(format(&api, 4096).await, Err(Problem::Failed));
    assert_eq!(requests.await.unwrap().len(), 3);
}

#[tokio::test]
async fn streaming_requires_completion_and_distinguishes_failures() {
    let server = Server::start().await;
    let api = server.api();
    let requests = server.serve(vec![
        Reply {
            delay: Duration::from_millis(10),
            ..Reply::event(OK_EVENTS)
        },
        Reply::event(DELTA_ONLY.to_owned() + USAGE_ERROR),
        Reply::event(DELTA_ONLY.to_owned() + "data: {\"type\":\"response.incomplete\"}\n\n"),
        Reply::event(DELTA_ONLY),
        Reply {
            extra_bytes: 64,
            ..Reply::event(DELTA_ONLY)
        },
        Reply::json(401, json!({"detail": "admission rejected"})),
        Reply::event(OK_EVENTS),
    ]);
    assert_eq!(format(&api, 4096).await.unwrap(), FORMATTED);
    for problem in [
        Problem::UsageLimit,
        Problem::Failed,
        Problem::Interrupted,
        Problem::Interrupted,
        Problem::SignInRequired,
    ] {
        assert_eq!(format(&api, 4096).await, Err(problem));
    }
    assert_eq!(format(&api, 32).await, Err(Problem::Failed));
    for request in requests.await.unwrap() {
        assert_eq!(request.line, "POST /v1/responses HTTP/1.1\r\n");
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["model"], "model");
        assert_eq!(
            body["input"],
            json!([{"role": "user", "content": CORRECTED}])
        );
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body.as_object().unwrap().len(), 5);
    }
}

#[test]
fn dictionary_checks_preserve_repeated_names() {
    let dictionary = dictionary();
    assert!(super::preserves_terms(&dictionary, CORRECTED, FORMATTED));
    assert!(!super::preserves_terms(&dictionary, "KoeとKoe", "Koe。"));
    assert!(!super::preserves_terms(&dictionary, CORRECTED, "コエ。"));
}

#[tokio::test]
async fn oauth_checks_state_and_does_not_change_the_saved_client() {
    let api = Api::new().unwrap();
    let host = "urn:uuid:32e01b6c-d89a-4c5b-815a-efca636e9947";
    let auth = Authorization::start(&api, host, None).await.unwrap();
    let params: std::collections::HashMap<_, _> = auth.url.query_pairs().collect();
    assert_eq!(params["client_id"], "dynamic_agent_client");
    assert_eq!(params["agent_name_hint"], "Koe");
    assert_eq!(params["ext_agent_host_id"], host);
    assert_eq!(params["code_challenge_method"], "S256");
    assert!(params["scope"].contains("chatgpt.tokens.use.direct"));

    async fn callback(redirect: &str, query: &str) -> TcpStream {
        let url = url::Url::parse(redirect).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", url.port().unwrap()))
            .await
            .unwrap();
        stream
            .write_all(
                format!(
                    "GET {}?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                    url.path()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream
    }

    let _wrong = callback(
        &params["redirect_uri"],
        "state=wrong&code=bogus&client_id=oaiapp_wrong",
    )
    .await;
    let _duplicate = callback(
        &params["redirect_uri"],
        &format!("state={0}&state={0}&code=bogus", params["state"]),
    )
    .await;
    let _right = callback(
        &params["redirect_uri"],
        &format!(
            "state={}&code=fixture-code&client_id=oaiapp_fixture",
            params["state"]
        ),
    )
    .await;
    let (id, code, _) = tokio::time::timeout(Duration::from_secs(3), auth.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(id, "oaiapp_fixture");
    assert_eq!(code, "fixture-code");

    let account = super::oauth::Registration {
        client_id: id,
        email: None,
        id_token: None,
        consent: false,
    };
    let auth = Authorization::start(&api, host, Some(&account))
        .await
        .unwrap();
    let params: std::collections::HashMap<_, _> = auth.url.query_pairs().collect();
    assert_eq!(params["client_id"], "oaiapp_fixture");
    assert!(!params.contains_key("agent_name_hint"));
    let _wrong_client = callback(
        &params["redirect_uri"],
        &format!(
            "state={}&code=fixture-code&client_id=oaiapp_other",
            params["state"]
        ),
    )
    .await;
    assert_eq!(auth.wait().await.unwrap_err(), Problem::InvalidSignIn);
}

#[tokio::test]
#[ignore = "requires the openssl executable to generate a synthetic identity key"]
async fn oidc_verifies_the_signature_and_claims() {
    use jsonwebtoken::{encode, jwk::Jwk, Algorithm, EncodingKey, Header};
    use std::process::{Command, Stdio};

    let mut generate = Command::new("openssl")
        .args(["genrsa", "-traditional", "2048"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let key = Command::new("openssl")
        .args(["rsa", "-traditional", "-outform", "DER"])
        .stdin(generate.stdout.take().unwrap())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(generate.wait().unwrap().success() && key.status.success());
    let key = EncodingKey::from_rsa_der(&key.stdout);
    let mut jwk = Jwk::from_encoding_key(&key, Algorithm::RS256).unwrap();
    jwk.common.key_id = Some("fixture-key".into());
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("fixture-key".into());

    let server = Server::start().await;
    let api = server.api();
    let oidc = json!({
        "issuer": api.issuer,
        "jwks_uri": format!("{}/keys", api.issuer),
        "revocation_endpoint": format!("{}/revoke", api.issuer)
    });
    let keys = json!({"keys": [jwk]});
    let mut replies = Vec::new();
    for _ in 0..6 {
        replies.extend([
            Reply::json(200, oidc.clone()),
            Reply::json(200, keys.clone()),
        ]);
    }
    let requests = server.serve(replies);

    let auth = Authorization::start(&api, "urn:uuid:fixture", None)
        .await
        .unwrap();
    let nonce = auth
        .url
        .query_pairs()
        .find(|(key, _)| key == "nonce")
        .unwrap()
        .1;
    let client_id = "oaiapp_fixture";
    let claims = json!({
        "sub": "fixture-subject", "aud": client_id, "iss": api.issuer, "exp": now() + 60,
        "nonce": nonce, "email": "koe@example.invalid"
    });
    let token = encode(&header, &claims, &key).unwrap();
    let identity = auth.identity(&api, client_id, &token).await.unwrap();
    assert_eq!(identity.subject, "fixture-subject");

    for (claim, bad_value) in [
        ("nonce", json!("wrong-nonce")),
        ("aud", json!("oaiapp_wrong")),
        ("iss", json!("https://wrong.example")),
        ("exp", json!(now() - 90)),
    ] {
        let mut altered = claims.clone();
        altered[claim] = bad_value;
        let token = encode(&header, &altered, &key).unwrap();
        assert!(matches!(
            auth.identity(&api, client_id, &token).await,
            Err(Problem::InvalidSignIn)
        ));
    }
    let (message, sig) = token.rsplit_once('.').unwrap();
    let changed = if sig.starts_with('A') { 'B' } else { 'A' };
    assert!(matches!(
        auth.identity(
            &api,
            client_id,
            &format!("{message}.{changed}{}", &sig[1..])
        )
        .await,
        Err(Problem::InvalidSignIn)
    ));
    assert_eq!(requests.await.unwrap().len(), 12);
}

#[tokio::test]
#[ignore = "requires a disposable D-Bus session with an unlocked GNOME keyring"]
async fn isolated_keyring_and_transcript_pipeline() {
    // This test must never be run against the desktop user's keyring.
    assert_eq!(
        std::env::var("KOE_CHATGPT_TEST_SESSION").as_deref(),
        Ok("disposable")
    );
    let server = Server::start().await;
    let api = server.api();
    let issuer = api.issuer.clone();
    let requests = server.serve(vec![
        Reply::json(
            200,
            json!({
                "access_token": "new-access", "refresh_token": "new-refresh",
                "token_type": "Bearer", "expires_in": 3600
            }),
        ),
        Reply {
            content_type: None,
            ..Reply::event(OK_EVENTS)
        },
        Reply::event(OK_EVENTS.replace("Koe", "コエ")),
        Reply {
            extra_bytes: 80,
            ..Reply::event(DELTA_ONLY)
        },
        Reply {
            delay: Duration::from_secs(9),
            ..Reply::event(DELTA_ONLY)
        },
        Reply::event(DELTA_ONLY.to_owned() + USAGE_ERROR),
        Reply::json(401, json!({"detail": "fixture authorization failed"})),
        Reply::json(
            200,
            json!({
                "issuer": issuer,
                "jwks_uri": format!("{issuer}/keys"),
                "revocation_endpoint": format!("{issuer}/revoke")
            }),
        ),
        Reply::event(""),
    ]);
    let chat = ChatGpt {
        api: Ok(api),
        store: Store::default(),
    };
    let processor = ChatGptProcessor {
        client: chat.clone(),
        max_response_bytes: 4096,
    };
    let (id, subject) = ("oaiapp_fixture", "fixture-subject");
    let fallback = |out: &crate::daemon::FinalTranscript| {
        out.fallback
            .as_ref()
            .and_then(|error| error.downcast_ref::<Problem>())
            .copied()
    };

    async fn check_paste(out: &crate::daemon::FinalTranscript, expected: &str) {
        assert_eq!(out.text, expected);
        if let Ok(window) = std::env::var("KOE_CHATGPT_PASTE_WINDOW") {
            out.paste(Some(&window), None, &Default::default(), false)
                .await
                .unwrap();
            // Leave enough time for the GTK test receiver to read the clipboard.
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    if let (Ok(model_path), Ok(wav_path)) = (
        std::env::var("KOE_SMOKE_WHISPER_MODEL"),
        std::env::var("KOE_SMOKE_WHISPER_WAV"),
    ) {
        use crate::recognition::{whisper_local::WhisperLocalRecognizer, SpeechRecognizer};
        let recognizer = WhisperLocalRecognizer::new(&crate::config::WhisperLocalConfig {
            model_path,
            language: "ja".into(),
        })
        .unwrap();
        let mut wav = hound::WavReader::open(wav_path).unwrap();
        assert_eq!(wav.spec().channels, 1);
        let audio = crate::audio::AudioData {
            sample_rate: wav.spec().sample_rate,
            samples: wav
                .samples::<i16>()
                .map(|sample| f32::from(sample.unwrap()) / 32768.0)
                .collect(),
        };
        let text = recognizer.transcribe(&audio).await.unwrap();
        assert!(text.contains("東京タワー"), "{text}");
        let mut dictionary = Dictionary::default();
        dictionary
            .terms
            .insert("東京タワー".into(), "Tokyo Tower".into());
        let out = format_transcript(
            &text,
            &processor,
            &WindowContext::default(),
            &dictionary,
            "",
            Ok(()),
        )
        .await;
        assert_eq!(fallback(&out), Some(Problem::NotConnected));
        assert_eq!(out.text, text.replace("東京タワー", "Tokyo Tower"));
        println!("Local Whisper synthetic speech: {text}");
    }

    let not_connected = utterance(&processor).await;
    assert_eq!(fallback(&not_connected), Some(Problem::NotConnected));
    check_paste(&not_connected, CORRECTED).await;

    let mut tx = chat.transaction().await.unwrap();
    tx.state.active = Some(id.to_owned());
    tx.state.accounts.push(Account {
        client_id: id.to_owned(),
        subject: subject.to_owned(),
        email: Some("koe@example.invalid".into()),
        model: Some("fixture-model".into()),
        generation: uuid::Uuid::new_v4(),
        tokens: Some(Tokens {
            access: "old-access".into(),
            refresh: Some("old-refresh".into()),
            id: Some("retained-id".into()),
            expires_at: now() - 60,
            scope: "resource.invoke chatgpt.tokens.use.direct".into(),
            earliest_refresh_at: None,
        }),
        block: None,
        welcomed: true,
    });
    chat.save(&tx).await.unwrap();
    assert!(matches!(chat.transaction().await, Err(Problem::Busy)));
    drop(tx);

    let output = utterance(&processor).await;
    assert!(output.fallback.is_none());
    assert!(output.learnings.is_empty());
    check_paste(&output, FORMATTED).await;

    let tx = chat.transaction().await.unwrap();
    let account = &tx.state.accounts[0];
    let tokens = account.tokens.as_ref().unwrap();
    assert_eq!(tokens.refresh.as_deref(), Some("new-refresh"));
    assert_eq!(tokens.id.as_deref(), Some("retained-id"));
    drop(tx);

    for problem in [
        Problem::DictionaryChanged,
        Problem::Interrupted,
        Problem::Timeout,
        Problem::UsageLimit,
        Problem::UsageLimit,
    ] {
        let output = utterance(&processor).await;
        assert_eq!(fallback(&output), Some(problem));
        check_paste(&output, CORRECTED).await;
    }

    let other_id = "oaiapp_other";
    let mut tx = chat.transaction().await.unwrap();
    tx.state.accounts.push(Account {
        client_id: other_id.into(),
        subject: subject.into(),
        email: Some("koe@example.invalid".into()),
        model: Some("other-model".into()),
        generation: uuid::Uuid::new_v4(),
        tokens: Some(Tokens {
            access: "other-access".into(),
            refresh: Some("other-refresh".into()),
            id: None,
            expires_at: now() + 1800,
            scope: "resource.invoke chatgpt.tokens.use.direct".into(),
            earliest_refresh_at: None,
        }),
        block: None,
        welcomed: true,
    });
    chat.save(&tx).await.unwrap();
    drop(tx);
    chat.activate(other_id).await.unwrap();
    let denied = utterance(&processor).await;
    assert_eq!(fallback(&denied), Some(Problem::SignInRequired));
    check_paste(&denied, CORRECTED).await;
    let settings = chat.settings().await.unwrap();
    assert_eq!(settings.active.as_deref(), Some(other_id));
    assert_ne!(settings.accounts[0].label, settings.accounts[1].label);
    assert!(settings.accounts[0].status == super::AccountStatus::UsageLimit);
    assert!(settings.accounts[1].status == super::AccountStatus::SignInRequired);
    chat.activate(id).await.unwrap();

    let lease = chat.authorize_request(true).await.unwrap();
    let tx = chat.transaction().await.unwrap();
    assert!(matches!(
        tx.state.accounts[0].block,
        Some(super::session::Block::UsageLimit)
    ));
    drop(tx);
    chat.disconnect(id).await.unwrap();
    let mut tx = chat.transaction().await.unwrap();
    assert!(matches!(
        ChatGpt::check_access(&lease, &mut tx.state),
        Err(Problem::NotConnected)
    ));
    assert!(tx.state.accounts[0].tokens.is_none());
    assert_eq!(tx.state.accounts[0].subject, subject);
    drop(tx);
    let signed_out = utterance(&processor).await;
    assert_eq!(fallback(&signed_out), Some(Problem::SignInRequired));
    check_paste(&signed_out, CORRECTED).await;

    let requests = requests.await.unwrap();
    let exchange = &requests[0];
    let form: std::collections::HashMap<_, _> =
        url::form_urlencoded::parse(&exchange.body).collect();
    assert_eq!(form["client_id"], id);
    assert_eq!(form["grant_type"], "refresh_token");
    assert_eq!(form["refresh_token"], "old-refresh");
    assert!(!form.contains_key("scope"));

    for request in &requests[1..6] {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(
            body["input"],
            json!([{"role": "user", "content": CORRECTED}])
        );
        assert_eq!(body.as_object().unwrap().len(), 5);
        assert!(request.headers.contains("authorization: Bearer new-access"));
    }
    let other = &requests[6];
    assert!(other.headers.contains("authorization: Bearer other-access"));
    let revoke = &requests[8];
    assert_eq!(revoke.line, "POST /revoke HTTP/1.1\r\n");
    let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&revoke.body).collect();
    assert_eq!(form["client_id"], id);
    assert_eq!(form["token"], "new-refresh");
    assert_eq!(form["token_type_hint"], "refresh_token");
}
