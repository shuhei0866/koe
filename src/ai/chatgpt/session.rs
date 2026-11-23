use std::fs::{File, OpenOptions, TryLockError};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::Duration;

use secret_service::{Collection, EncryptionType, SecretService};
use serde::{Deserialize, Serialize};

use super::{Outcome, Problem};

const KEYRING_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default, Serialize, Deserialize)]
pub(super) struct State {
    pub host_id: String,
    pub revision: u64,
    pub active: Option<String>,
    pub accounts: Vec<Account>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Account {
    pub client_id: String,
    pub subject: String,
    pub email: Option<String>,
    pub generation: uuid::Uuid,
    pub tokens: Option<Tokens>,
    pub model: Option<String>,
    pub block: Option<Block>,
    pub welcomed: bool,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum Block {
    UsageLimit,
    SignInRequired,
    Restricted,
    RetryAfter(i64),
}

#[derive(Serialize, Deserialize)]
pub(super) struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    pub id: Option<String>,
    pub scope: String,
    pub expires_at: i64,
    pub earliest_refresh_at: Option<i64>,
}

impl Tokens {
    pub fn allows_plan(&self) -> bool {
        let mut scopes = self.scope.split_ascii_whitespace();
        scopes.clone().any(|s| s == "chatgpt.tokens.use.direct")
            && scopes.any(|s| s == "resource.invoke")
    }
}

impl State {
    pub fn account_mut(&mut self, client_id: &str) -> Outcome<&mut Account> {
        self.accounts
            .iter_mut()
            .find(|account| account.client_id == client_id)
            .ok_or(Problem::NotConnected)
    }
}

#[derive(Clone)]
pub(super) struct Store {
    root: Option<PathBuf>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            // All Koe processes for this OS user share the same Secret Service
            // item, even if started with different XDG configuration paths.
            root: dirs::home_dir().map(|home| home.join(".local/share/koe/chatgpt-locks")),
        }
    }
}

impl Store {
    pub fn lock(&self, name: &str) -> Outcome<File> {
        let root = self.root.as_ref().ok_or(Problem::Keyring)?;
        std::fs::create_dir_all(root).map_err(|_| Problem::Keyring)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(root.join(name))
            .map_err(|_| Problem::Keyring)?;
        file.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Problem::Busy,
            TryLockError::Error(_) => Problem::Keyring,
        })?;
        Ok(file)
    }

    pub async fn connect(&self) -> Outcome<SecretService<'static>> {
        tokio::time::timeout(KEYRING_TIMEOUT, SecretService::connect(EncryptionType::Dh))
            .await
            .map_err(|_| Problem::Keyring)?
            .map_err(|_| Problem::Keyring)
    }

    pub async fn load(&self, service: &SecretService<'_>) -> Outcome<State> {
        tokio::time::timeout(KEYRING_TIMEOUT, async {
            let collection = collection(service).await?;
            let items = collection
                .search_items(attributes())
                .await
                .map_err(|_| Problem::Keyring)?;
            match items.as_slice() {
                [] => Ok(State::default()),
                [item] if !item.is_locked().await.map_err(|_| Problem::Keyring)? => {
                    let bytes = item.get_secret().await.map_err(|_| Problem::Keyring)?;
                    serde_json::from_slice(&bytes).map_err(|_| Problem::Keyring)
                }
                _ => Err(Problem::Keyring),
            }
        })
        .await
        .map_err(|_| Problem::Keyring)?
    }

    pub async fn save(&self, service: &SecretService<'_>, state: &State) -> Outcome<()> {
        tokio::time::timeout(KEYRING_TIMEOUT, async {
            let collection = collection(service).await?;
            let bytes = serde_json::to_vec(state).map_err(|_| Problem::Keyring)?;
            // Replace one Secret Service item so a rotated token pair and its
            // expiry cannot be split across writes. Accounts have distinct records.
            collection
                .create_item(
                    "Koe — ChatGPT accounts",
                    attributes(),
                    &bytes,
                    true,
                    "application/json",
                )
                .await
                .map_err(|_| Problem::Keyring)?;
            Ok(())
        })
        .await
        .map_err(|_| Problem::Keyring)?
    }
}

async fn collection<'a>(service: &'a SecretService<'_>) -> Outcome<Collection<'a>> {
    let collection = service
        .get_default_collection()
        .await
        .map_err(|_| Problem::Keyring)?;
    if collection.is_locked().await.map_err(|_| Problem::Keyring)? {
        return Err(Problem::Keyring);
    }
    Ok(collection)
}

fn attributes() -> std::collections::HashMap<&'static str, &'static str> {
    [("service", "koe"), ("key", "chatgpt-plan-accounts-v1")].into()
}
