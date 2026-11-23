mod api;
#[cfg(feature = "gui")]
mod oauth;
mod session;

use std::fs::File;

use async_trait::async_trait;
use secret_service::SecretService;
use session::{Block, State, Store, Tokens};

use super::{ConsolidationResult, ProcessResult, TextProcessor};
use crate::{context::WindowContext, dictionary::Dictionary};
use api::Api;
#[cfg(feature = "gui")]
pub use api::Model;

type Outcome<T> = Result<T, Problem>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Problem {
    #[error("ChatGPT is disconnected")]
    NotConnected,
    #[error("Sign in to ChatGPT again")]
    SignInRequired,
    #[error("ChatGPT plan usage needs permission")]
    PermissionNeeded,
    #[error("ChatGPT usage limit reached")]
    UsageLimit,
    #[error("ChatGPT plan usage is restricted for this account or workspace")]
    Restricted,
    #[error("ChatGPT is temporarily unavailable")]
    Unavailable,
    #[error("ChatGPT is rate limiting requests")]
    RateLimited,
    #[error("The ChatGPT request timed out")]
    Timeout,
    #[error("ChatGPT formatting was interrupted")]
    Interrupted,
    #[error("ChatGPT did not return a usable transcript")]
    Failed,
    #[error("ChatGPT changed a dictionary term")]
    DictionaryChanged,
    #[error("The selected ChatGPT model is unavailable. Refresh models in Settings")]
    ModelUnavailable,
    #[error("ChatGPT formatting requires local Whisper")]
    LocalWhisperRequired,
    #[error("The GNOME login keyring is locked or unavailable")]
    Keyring,
    #[error("Another ChatGPT account operation is running")]
    Busy,
    #[error("ChatGPT sign-in could not be verified")]
    InvalidSignIn,
    #[cfg(feature = "gui")]
    #[error("ChatGPT sign-in permission was declined")]
    ConsentDenied,
    #[cfg(feature = "gui")]
    #[error("Could not open the ChatGPT browser sign-in")]
    Browser,
    #[cfg(feature = "gui")]
    #[error("Disconnected locally. Check ChatGPT Settings to revoke access there")]
    RevocationUnconfirmed,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Clone)]
pub struct ChatGpt {
    api: Outcome<Api>,
    store: Store,
}

impl Default for ChatGpt {
    fn default() -> Self {
        Self {
            api: Api::new(),
            store: Store::default(),
        }
    }
}

struct Transaction {
    _lock: File,
    service: SecretService<'static>,
    state: State,
}

struct Access {
    client_id: String,
    generation: uuid::Uuid,
    token: String,
    model: Option<String>,
}

impl ChatGpt {
    fn api(&self) -> Outcome<&Api> {
        self.api.as_ref().map_err(|problem| *problem)
    }

    async fn transaction(&self) -> Outcome<Transaction> {
        let lock = self.store.lock("accounts.lock")?;
        let service = self.store.connect().await?;
        let state = self.store.load(&service).await?;
        Ok(Transaction {
            _lock: lock,
            service,
            state,
        })
    }

    #[cfg(feature = "gui")]
    async fn wait_for_transaction(&self) -> Outcome<Transaction> {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                match self.transaction().await {
                    Err(Problem::Busy) => {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await
                    }
                    result => return result,
                }
            }
        })
        .await
        .map_err(|_| Problem::Busy)?
    }

    async fn save(&self, tx: &Transaction) -> Outcome<()> {
        self.store.save(&tx.service, &tx.state).await
    }

    async fn authorize_request(&self, resume_usage: bool) -> Outcome<Access> {
        let mut tx = self.transaction().await?;
        let client_id = tx.state.active.clone().ok_or(Problem::NotConnected)?;
        let account = tx.state.account_mut(&client_id)?;
        if !resume_usage {
            match account.block {
                Some(Block::UsageLimit) => return Err(Problem::UsageLimit),
                Some(Block::SignInRequired) => return Err(Problem::SignInRequired),
                Some(Block::Restricted) => return Err(Problem::Restricted),
                Some(Block::RetryAfter(until)) if now() < until => {
                    return Err(Problem::Unavailable)
                }
                _ => {}
            }
        }
        let tokens = account.tokens.as_ref().ok_or(Problem::SignInRequired)?;
        if !tokens.allows_plan() {
            return Err(Problem::PermissionNeeded);
        }
        // Honor the token endpoint's earliest refresh time. An access token in
        // its final seconds is still usable if the server won't refresh it yet.
        let time = now();
        if tokens.expires_at <= time + 30
            && tokens
                .earliest_refresh_at
                .is_none_or(|earliest| earliest <= time)
        {
            let outcome = self.refresh(&client_id, tokens).await;
            match outcome {
                Ok(tokens) => {
                    account.tokens = Some(tokens);
                    self.save(&tx).await?;
                }
                Err(Problem::SignInRequired) => {
                    account.tokens = None;
                    account.generation = uuid::Uuid::new_v4();
                    self.save(&tx).await?;
                    return Err(Problem::SignInRequired);
                }
                Err(problem @ (Problem::Unavailable | Problem::Timeout)) => {
                    if account.block.is_none()
                        || matches!(account.block, Some(Block::RetryAfter(_)))
                    {
                        account.block = Some(Block::RetryAfter(now() + 15));
                        self.save(&tx).await?;
                    }
                    return Err(problem);
                }
                Err(problem) => return Err(problem),
            }
        }
        let account = tx.state.account_mut(&client_id)?;
        let tokens = account.tokens.as_ref().ok_or(Problem::SignInRequired)?;
        if !tokens.allows_plan() {
            return Err(Problem::PermissionNeeded);
        }
        if tokens.expires_at <= now() {
            return Err(Problem::SignInRequired);
        }
        Ok(Access {
            client_id,
            generation: account.generation,
            token: tokens.access.clone(),
            model: account.model.clone(),
        })
    }

    async fn refresh(&self, client_id: &str, tokens: &Tokens) -> Outcome<Tokens> {
        let api = self.api()?;
        let refresh = tokens.refresh.as_deref().ok_or(Problem::SignInRequired)?;
        api.token(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh),
            ("resource", &api.resource),
        ])
        .await?
        .into_tokens(Some(tokens))
    }

    fn check_access<'a>(
        access: &Access,
        state: &'a mut State,
    ) -> Outcome<&'a mut session::Account> {
        if state.active.as_deref() != Some(&access.client_id) {
            return Err(Problem::NotConnected);
        }
        let account = state.account_mut(&access.client_id)?;
        if account.generation != access.generation {
            return Err(Problem::NotConnected);
        }
        Ok(account)
    }

    async fn record_error(&self, access: &Access, problem: Problem) {
        let block = match problem {
            Problem::UsageLimit => Block::UsageLimit,
            Problem::SignInRequired => Block::SignInRequired,
            Problem::Restricted => Block::Restricted,
            Problem::Unavailable | Problem::RateLimited => Block::RetryAfter(now() + 15),
            _ => return,
        };
        let result: Outcome<()> = async {
            let mut tx = self.transaction().await?;
            let account = Self::check_access(access, &mut tx.state)?;
            if !matches!(
                account.block,
                Some(Block::UsageLimit | Block::SignInRequired | Block::Restricted)
            ) || !matches!(block, Block::RetryAfter(_))
            {
                account.block = Some(block);
                self.save(&tx).await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            tracing::debug!(%error, "Could not save ChatGPT availability");
        }
    }

    async fn format(&self, input: &str, max_bytes: usize) -> Outcome<String> {
        let api = self.api()?;
        let access = self.authorize_request(false).await?;
        let result = async {
            let (mut model, resolved) = match &access.model {
                Some(model) => (model.clone(), false),
                None => {
                    let model = api
                        .models(&access.token)
                        .await?
                        .into_iter()
                        .next()
                        .ok_or(Problem::ModelUnavailable)?;
                    (model.slug, true)
                }
            };
            let (response, started) = {
                // Start the request under the session lock so a completed
                // disconnect cannot be followed by a request with old credentials.
                let mut tx = self.transaction().await?;
                let account = Self::check_access(&access, &mut tx.state)?;
                if let Some(current) = &account.model {
                    model.clone_from(current);
                } else if resolved {
                    account.model = Some(model.clone());
                    self.save(&tx).await?;
                }
                tracing::info!(%model, "Requesting ChatGPT formatting");
                let started = std::time::Instant::now();
                (
                    api.start_format(&access.token, &model, input).await?,
                    started,
                )
            };
            let formatted = Api::finish_format(response, max_bytes, started).await?;
            let mut tx = self.transaction().await?;
            Self::check_access(&access, &mut tx.state)?;
            Ok(formatted)
        }
        .await;
        if let Err(problem) = result {
            self.record_error(&access, problem).await;
        }
        result
    }
}

pub struct ChatGptProcessor {
    client: ChatGpt,
    max_response_bytes: usize,
}

impl ChatGptProcessor {
    pub fn new(max_response_bytes: usize) -> Self {
        Self {
            client: ChatGpt::default(),
            max_response_bytes,
        }
    }
}

#[async_trait]
impl TextProcessor for ChatGptProcessor {
    async fn process(
        &self,
        corrected: &str,
        _context: &WindowContext,
        dictionary: &Dictionary,
        _memory_context: &str,
    ) -> anyhow::Result<ProcessResult> {
        let text = self
            .client
            .format(corrected, self.max_response_bytes)
            .await?;
        if !preserves_terms(dictionary, corrected, &text) {
            return Err(Problem::DictionaryChanged.into());
        }
        Ok(ProcessResult {
            text,
            learnings: Vec::new(),
        })
    }

    async fn consolidate_memory(&self, _: &str) -> anyhow::Result<Option<ConsolidationResult>> {
        Ok(None)
    }
}

fn preserves_terms(dictionary: &Dictionary, corrected: &str, formatted: &str) -> bool {
    dictionary
        .terms
        .values()
        .filter(|term| !term.is_empty())
        .all(|term| formatted.matches(term).count() >= corrected.matches(term).count())
}

#[cfg(feature = "gui")]
mod account_actions;
#[cfg(feature = "gui")]
pub use account_actions::{AccountStatus, SettingsState};

#[cfg(all(test, feature = "gui"))]
mod tests;
