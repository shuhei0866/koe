use super::{
    api::Model,
    oauth::{finish, Authorization, Registration},
    session::{Account, Block, State},
    ChatGpt, Outcome, Problem,
};

pub struct AccountSummary {
    pub client_id: String,
    pub label: String,
    pub status: AccountStatus,
    pub has_tokens: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum AccountStatus {
    Connected,
    SignedOut,
    PermissionNeeded,
    UsageLimit,
    SignInRequired,
    Restricted,
}

impl AccountStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Using ChatGPT plan",
            Self::SignedOut => "Disconnected",
            Self::PermissionNeeded => "ChatGPT plan usage needs permission",
            Self::UsageLimit => "ChatGPT usage limit reached",
            Self::SignInRequired => "Sign in to ChatGPT again",
            Self::Restricted => "ChatGPT plan usage is restricted",
        }
    }

    pub fn can_use(self) -> bool {
        matches!(self, Self::Connected | Self::UsageLimit | Self::Restricted)
    }
}

pub struct SettingsState {
    pub accounts: Vec<AccountSummary>,
    pub active: Option<String>,
    pub model: Option<String>,
}

impl ChatGpt {
    pub async fn settings(&self) -> Outcome<SettingsState> {
        let state = self.transaction().await?.state;
        let model = state
            .accounts
            .iter()
            .find(|a| Some(&a.client_id) == state.active.as_ref())
            .and_then(|a| a.model.clone());
        let accounts = state
            .accounts
            .into_iter()
            .enumerate()
            .map(|(index, a)| {
                let status = match (&a.tokens, a.block) {
                    (None, _) => AccountStatus::SignedOut,
                    (Some(tokens), _) if !tokens.allows_plan() => AccountStatus::PermissionNeeded,
                    (_, Some(Block::UsageLimit)) => AccountStatus::UsageLimit,
                    (_, Some(Block::SignInRequired)) => AccountStatus::SignInRequired,
                    (_, Some(Block::Restricted)) => AccountStatus::Restricted,
                    _ => AccountStatus::Connected,
                };
                AccountSummary {
                    client_id: a.client_id,
                    label: format!(
                        "{} · {}",
                        a.email.as_deref().unwrap_or("ChatGPT account"),
                        index + 1
                    ),
                    status,
                    has_tokens: a.tokens.is_some(),
                }
            })
            .collect();
        Ok(SettingsState {
            accounts,
            active: state.active,
            model,
        })
    }

    pub async fn activate(&self, client_id: &str) -> Outcome<()> {
        let mut tx = self.transaction().await?;
        let account = tx.state.account_mut(client_id)?;
        if account.tokens.is_none() {
            return Err(Problem::SignInRequired);
        }
        // A lease taken before switching away must also fail after switching back.
        account.generation = uuid::Uuid::new_v4();
        tx.state.active = Some(client_id.to_owned());
        tx.state.revision += 1;
        self.save(&tx).await
    }

    pub async fn models(&self, resume_usage: bool) -> Outcome<(Vec<Model>, Option<String>)> {
        let access = self.authorize_request(resume_usage).await?;
        let models = match self.api()?.models(&access.token).await {
            Ok(models) => models,
            Err(problem) => {
                self.record_error(&access, problem).await;
                return Err(problem);
            }
        };
        let mut tx = self.transaction().await?;
        let account = Self::check_access(&access, &mut tx.state)?;
        let old = account.model.clone();
        if !models
            .iter()
            .any(|m| Some(&m.slug) == account.model.as_ref())
        {
            account.model = models.first().map(|m| m.slug.clone());
        }
        let chosen = account.model.clone();
        if (resume_usage && account.block.take().is_some()) || chosen != old {
            self.save(&tx).await?;
        }
        Ok((models, chosen))
    }

    pub async fn set_model(&self, client_id: &str, slug: String) -> Outcome<()> {
        let mut tx = self.transaction().await?;
        if tx.state.active.as_deref() != Some(client_id) {
            return Err(Problem::NotConnected);
        }
        let account = tx.state.account_mut(client_id)?;
        if account.tokens.is_none() {
            return Err(Problem::SignInRequired);
        }
        account.model = Some(slug);
        self.save(&tx).await
    }

    pub async fn disconnect(&self, client_id: &str) -> Outcome<()> {
        let mut tx = self.wait_for_transaction().await?;
        let account = tx.state.account_mut(client_id)?;
        let revoke =
            if let Some(refresh) = account.tokens.as_ref().and_then(|t| t.refresh.as_deref()) {
                match self.api() {
                    Ok(api) => api.revoke(client_id, refresh).await,
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            };
        account.tokens = None;
        account.block = None;
        account.generation = uuid::Uuid::new_v4();
        tx.state.revision += 1;
        self.save(&tx).await?;
        revoke.map_err(|_| Problem::RevocationUnconfirmed)
    }

    pub async fn sign_in(
        &self,
        selected: Option<&str>,
        open_browser: impl Fn(&str) -> Outcome<()>,
    ) -> Outcome<bool> {
        let _sign_in = self.store.lock("sign-in.lock")?;
        let api = self.api()?;
        let (host, revision, mut registration) = {
            let mut tx = self.wait_for_transaction().await?;
            if tx.state.host_id.is_empty() {
                tx.state.host_id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
                self.save(&tx).await?;
            }
            let registration = selected
                .map(|client_id| {
                    let account = tx.state.account_mut(client_id)?;
                    Ok(Registration {
                        client_id: account.client_id.clone(),
                        email: account.email.clone(),
                        id_token: account.tokens.as_ref().and_then(|tokens| tokens.id.clone()),
                        consent: account
                            .tokens
                            .as_ref()
                            .is_some_and(|tokens| !tokens.allows_plan()),
                    })
                })
                .transpose()?;
            (tx.state.host_id, tx.state.revision, registration)
        };

        for attempt in 0..2 {
            let auth = Authorization::start(api, &host, registration.as_ref()).await?;
            open_browser(auth.url.as_str())?;
            let (client_id, code, socket) = auth.wait().await?;
            let result = async {
                let mut tx = self.wait_for_transaction().await?;
                if tx.state.revision != revision {
                    return Err(Problem::Busy);
                }
                let reply = auth.exchange(api, &client_id, &code).await?;
                let id = reply.id_token.as_deref().ok_or(Problem::InvalidSignIn)?;
                let identity = auth.identity(api, &client_id, id).await?;
                let first_use = save_registration(
                    &mut tx.state,
                    &client_id,
                    identity,
                    reply.into_tokens(None)?,
                )?;
                self.save(&tx).await?;
                Ok(first_use)
            }
            .await;
            finish(socket, result.is_ok()).await;
            if result == Err(Problem::SignInRequired) && attempt == 0 {
                registration.get_or_insert(Registration {
                    client_id,
                    email: None,
                    id_token: None,
                    consent: false,
                });
                continue;
            }
            return result;
        }
        Err(Problem::InvalidSignIn)
    }
}

fn save_registration(
    state: &mut State,
    client_id: &str,
    identity: super::oauth::Identity,
    tokens: super::session::Tokens,
) -> Outcome<bool> {
    let enabled = tokens.allows_plan();
    if enabled && tokens.refresh.is_none() {
        return Err(Problem::InvalidSignIn);
    }
    let new_session = uuid::Uuid::new_v4();
    let first_use = if let Ok(account) = state.account_mut(client_id) {
        if account.subject != identity.subject {
            return Err(Problem::InvalidSignIn);
        }
        let first_use = enabled && !account.welcomed;
        account.email = identity.email;
        account.tokens = Some(tokens);
        account.generation = new_session;
        account.block = None;
        account.welcomed |= enabled;
        first_use
    } else {
        state.accounts.push(Account {
            client_id: client_id.to_owned(),
            subject: identity.subject,
            email: identity.email,
            tokens: Some(tokens),
            generation: new_session,
            model: None,
            block: None,
            welcomed: enabled,
        });
        enabled
    };
    state.active = Some(client_id.to_owned());
    state.revision += 1;
    Ok(first_use)
}
