use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{App, AppHandle, Emitter, Runtime};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_shell::ShellExt;
use url::Url;

pub const DESKTOP_OAUTH_REDIRECT_URI: &str = "scriptum://auth/callback";
pub(crate) const DEEP_LINK_EVENT: &str = "scriptum://auth/deep-link";
const KEYRING_SERVICE: &str = "com.scriptum.desktop";
const KEYRING_ACCOUNT: &str = "oauth_tokens";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub access_expires_at: Option<String>,
    #[serde(default)]
    pub refresh_expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthCallbackPayload {
    pub url: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

pub fn init_deep_link_bridge<R: Runtime>(app: &App<R>) {
    let app_handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        let urls: Vec<String> = event.urls().into_iter().map(|url| url.to_string()).collect();
        let _ = app_handle.emit(DEEP_LINK_EVENT, urls);
    });

    if let Ok(Some(urls)) = app.deep_link().get_current() {
        let urls: Vec<String> = urls.into_iter().map(|url| url.to_string()).collect();
        let _ = app.emit(DEEP_LINK_EVENT, urls);
    }
}

pub fn open_authorization_url<R: Runtime>(
    app: &AppHandle<R>,
    authorization_url: &str,
) -> Result<()> {
    let parsed = Url::parse(authorization_url).context("authorization URL must be valid")?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(anyhow!("authorization URL must use http or https, got `{other}`")),
    }

    #[allow(deprecated)]
    app.shell()
        .open(parsed.to_string(), None)
        .context("failed to open authorization URL in system browser")?;
    Ok(())
}

pub fn parse_oauth_callback(url: &str) -> Result<OAuthCallbackPayload> {
    let parsed = Url::parse(url).context("callback URL must be valid")?;
    if parsed.scheme() != "scriptum" {
        return Err(anyhow!("callback URL must use `scriptum` scheme, got `{}`", parsed.scheme()));
    }
    if parsed.host_str() != Some("auth") {
        return Err(anyhow!("callback URL host must be `auth`"));
    }
    if parsed.path() != "/callback" {
        return Err(anyhow!("callback URL path must be `/callback`"));
    }

    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_description = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => error_description = Some(value.into_owned()),
            _ => {}
        }
    }

    Ok(OAuthCallbackPayload { url: parsed.to_string(), code, state, error, error_description })
}

pub fn store_tokens(tokens: &AuthTokens) -> Result<()> {
    store_tokens_with_store(&KeyringSecretStore, tokens)
}

pub fn load_tokens() -> Result<Option<AuthTokens>> {
    load_tokens_with_store(&KeyringSecretStore)
}

pub fn clear_tokens() -> Result<()> {
    clear_tokens_with_store(&KeyringSecretStore)
}

fn store_tokens_with_store(store: &dyn SecretStore, tokens: &AuthTokens) -> Result<()> {
    if tokens.access_token.trim().is_empty() {
        return Err(anyhow!("access token must not be empty"));
    }
    if tokens.refresh_token.trim().is_empty() {
        return Err(anyhow!("refresh token must not be empty"));
    }

    let serialized = serde_json::to_string(tokens).context("failed to serialize auth tokens")?;
    store
        .set_secret(KEYRING_SERVICE, KEYRING_ACCOUNT, &serialized)
        .context("failed to persist auth tokens to keychain")
}

fn load_tokens_with_store(store: &dyn SecretStore) -> Result<Option<AuthTokens>> {
    let Some(serialized) = store
        .get_secret(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .context("failed to read auth tokens from keychain")?
    else {
        return Ok(None);
    };

    let tokens =
        serde_json::from_str::<AuthTokens>(&serialized).context("failed to decode auth tokens")?;
    Ok(Some(tokens))
}

fn clear_tokens_with_store(store: &dyn SecretStore) -> Result<()> {
    store
        .delete_secret(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .context("failed to clear auth tokens from keychain")
}

trait SecretStore: Send + Sync {
    fn set_secret(&self, service: &str, account: &str, value: &str) -> Result<()>;
    fn get_secret(&self, service: &str, account: &str) -> Result<Option<String>>;
    fn delete_secret(&self, service: &str, account: &str) -> Result<()>;
}

struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn set_secret(&self, service: &str, account: &str, value: &str) -> Result<()> {
        let entry =
            keyring::Entry::new(service, account).context("failed to initialize keychain entry")?;
        entry.set_password(value).context("failed to write keychain entry")?;
        Ok(())
    }

    fn get_secret(&self, service: &str, account: &str) -> Result<Option<String>> {
        let entry =
            keyring::Entry::new(service, account).context("failed to initialize keychain entry")?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error).context("failed to read keychain entry"),
        }
    }

    fn delete_secret(&self, service: &str, account: &str) -> Result<()> {
        let entry =
            keyring::Entry::new(service, account).context("failed to initialize keychain entry")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error).context("failed to delete keychain entry"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemorySecretStore {
        values: Mutex<HashMap<(String, String), String>>,
    }

    impl SecretStore for MemorySecretStore {
        fn set_secret(&self, service: &str, account: &str, value: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap()
                .insert((service.to_string(), account.to_string()), value.to_string());
            Ok(())
        }

        fn get_secret(&self, service: &str, account: &str) -> Result<Option<String>> {
            Ok(self
                .values
                .lock()
                .unwrap()
                .get(&(service.to_string(), account.to_string()))
                .cloned())
        }

        fn delete_secret(&self, service: &str, account: &str) -> Result<()> {
            self.values.lock().unwrap().remove(&(service.to_string(), account.to_string()));
            Ok(())
        }
    }

    #[test]
    fn parse_oauth_callback_extracts_code_and_state() {
        let payload =
            parse_oauth_callback("scriptum://auth/callback?code=abc123&state=xyz").unwrap();
        assert_eq!(payload.code.as_deref(), Some("abc123"));
        assert_eq!(payload.state.as_deref(), Some("xyz"));
        assert_eq!(payload.error, None);
    }

    #[test]
    fn parse_oauth_callback_extracts_error_fields() {
        let payload = parse_oauth_callback(
            "scriptum://auth/callback?error=access_denied&error_description=user+cancelled",
        )
        .unwrap();
        assert_eq!(payload.error.as_deref(), Some("access_denied"));
        assert_eq!(payload.error_description.as_deref(), Some("user cancelled"));
        assert_eq!(payload.code, None);
    }

    #[test]
    fn parse_oauth_callback_rejects_non_scriptum_scheme() {
        let error = parse_oauth_callback("https://example.com/callback?code=abc").unwrap_err();
        assert!(error.to_string().contains("scriptum"));
    }

    #[test]
    fn token_storage_round_trip() {
        let store = MemorySecretStore::default();
        let tokens = AuthTokens {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            access_expires_at: Some("2026-02-08T00:00:00Z".to_string()),
            refresh_expires_at: Some("2026-03-08T00:00:00Z".to_string()),
        };

        store_tokens_with_store(&store, &tokens).unwrap();
        let loaded = load_tokens_with_store(&store).unwrap().unwrap();
        assert_eq!(loaded, tokens);

        clear_tokens_with_store(&store).unwrap();
        assert_eq!(load_tokens_with_store(&store).unwrap(), None);
    }

    #[test]
    fn store_tokens_rejects_empty_values() {
        let store = MemorySecretStore::default();
        let tokens = AuthTokens {
            access_token: "   ".to_string(),
            refresh_token: "refresh".to_string(),
            access_expires_at: None,
            refresh_expires_at: None,
        };

        let error = store_tokens_with_store(&store, &tokens).unwrap_err();
        assert!(error.to_string().contains("access token"));
    }
}
