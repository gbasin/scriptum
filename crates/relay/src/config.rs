// Relay server configuration.
//
// Centralizes environment variable parsing with defaults for local
// development. Individual modules (OAuth, DB pool, etc.) may still read
// their own env vars — this module covers the core server settings.

use std::net::SocketAddr;

/// Core relay server configuration.
///
/// Constructed via [`RelayConfig::from_env`] which reads environment
/// variables and falls back to sensible development defaults.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Listen address (host:port).
    pub listen_addr: SocketAddr,
    /// JWT signing secret for access tokens.
    pub jwt_secret: String,
    /// Base URL for WebSocket connections (e.g. `ws://localhost:8080`).
    pub ws_base_url: String,
    /// PostgreSQL connection string.
    pub database_url: Option<String>,
    /// Comma-separated CORS origins (or `"*"` for any).
    pub cors_origins: Option<String>,
    /// Log filter directive (e.g. `info`, `scriptum_relay=debug`).
    pub log_filter: String,
    /// Share link base URL for generating share links.
    pub share_link_base_url: String,
}

impl RelayConfig {
    /// Parse configuration from environment variables.
    ///
    /// | Variable | Default |
    /// |---|---|
    /// | `SCRIPTUM_RELAY_HOST` | `0.0.0.0` |
    /// | `SCRIPTUM_RELAY_PORT` | `8080` |
    /// | `SCRIPTUM_RELAY_JWT_SECRET` | dev-only placeholder |
    /// | `SCRIPTUM_RELAY_WS_BASE_URL` | `ws://{host}:{port}` |
    /// | `SCRIPTUM_RELAY_DATABASE_URL` | *(none)* |
    /// | `SCRIPTUM_RELAY_CORS_ORIGINS` | *(none — cors.rs uses dev defaults)* |
    /// | `SCRIPTUM_RELAY_LOG_FILTER` | `info` |
    /// | `SCRIPTUM_RELAY_SHARE_LINK_BASE_URL` | `http://localhost:3000/share` |
    pub fn from_env() -> Self {
        Self::from_env_fn(|key| std::env::var(key))
    }

    /// Testable constructor that accepts an environment lookup function.
    fn from_env_fn<F>(env: F) -> Self
    where
        F: Fn(&str) -> Result<String, std::env::VarError>,
    {
        let host = env("SCRIPTUM_RELAY_HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port: u16 = env("SCRIPTUM_RELAY_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);
        let listen_addr = format!("{host}:{port}")
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], port)));

        let jwt_secret = env("SCRIPTUM_RELAY_JWT_SECRET").unwrap_or_else(|_| {
            "scriptum_local_development_jwt_secret_must_be_32_chars".into()
        });

        let ws_base_url = env("SCRIPTUM_RELAY_WS_BASE_URL")
            .unwrap_or_else(|_| format!("ws://{listen_addr}"));

        let database_url = env("SCRIPTUM_RELAY_DATABASE_URL").ok();
        let cors_origins = env("SCRIPTUM_RELAY_CORS_ORIGINS").ok();

        let log_filter =
            env("SCRIPTUM_RELAY_LOG_FILTER").unwrap_or_else(|_| "info".into());

        let share_link_base_url = env("SCRIPTUM_RELAY_SHARE_LINK_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:3000/share".into());

        Self {
            listen_addr,
            jwt_secret,
            ws_base_url,
            database_url,
            cors_origins,
            log_filter,
            share_link_base_url,
        }
    }

    /// Returns true when using the development-only JWT secret.
    pub fn is_dev_jwt_secret(&self) -> bool {
        self.jwt_secret == "scriptum_local_development_jwt_secret_must_be_32_chars"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_from_map(
        map: HashMap<&'static str, &'static str>,
    ) -> impl Fn(&str) -> Result<String, std::env::VarError> {
        move |key: &str| {
            map.get(key)
                .map(|v| v.to_string())
                .ok_or(std::env::VarError::NotPresent)
        }
    }

    #[test]
    fn defaults_when_no_env_vars() {
        let cfg = RelayConfig::from_env_fn(env_from_map(HashMap::new()));
        assert_eq!(cfg.listen_addr.port(), 8080);
        assert_eq!(cfg.listen_addr.ip().to_string(), "0.0.0.0");
        assert!(cfg.is_dev_jwt_secret());
        assert_eq!(cfg.ws_base_url, "ws://0.0.0.0:8080");
        assert!(cfg.database_url.is_none());
        assert!(cfg.cors_origins.is_none());
        assert_eq!(cfg.log_filter, "info");
        assert_eq!(cfg.share_link_base_url, "http://localhost:3000/share");
    }

    #[test]
    fn custom_port() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_PORT", "9090");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert_eq!(cfg.listen_addr.port(), 9090);
        assert_eq!(cfg.ws_base_url, "ws://0.0.0.0:9090");
    }

    #[test]
    fn custom_host_and_port() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_HOST", "127.0.0.1");
        m.insert("SCRIPTUM_RELAY_PORT", "3000");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert_eq!(cfg.listen_addr.to_string(), "127.0.0.1:3000");
    }

    #[test]
    fn custom_jwt_secret_is_not_dev() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_JWT_SECRET", "production_secret_at_least_32_chars!!");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert!(!cfg.is_dev_jwt_secret());
        assert_eq!(cfg.jwt_secret, "production_secret_at_least_32_chars!!");
    }

    #[test]
    fn ws_base_url_override() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_WS_BASE_URL", "wss://relay.scriptum.dev");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert_eq!(cfg.ws_base_url, "wss://relay.scriptum.dev");
    }

    #[test]
    fn database_url_from_env() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_DATABASE_URL", "postgres://u:p@host/db");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert_eq!(cfg.database_url.as_deref(), Some("postgres://u:p@host/db"));
    }

    #[test]
    fn cors_origins_from_env() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_CORS_ORIGINS", "https://app.scriptum.dev");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert_eq!(cfg.cors_origins.as_deref(), Some("https://app.scriptum.dev"));
    }

    #[test]
    fn log_filter_override() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_LOG_FILTER", "debug,tower_http=trace");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert_eq!(cfg.log_filter, "debug,tower_http=trace");
    }

    #[test]
    fn invalid_port_uses_default() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_PORT", "not_a_number");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert_eq!(cfg.listen_addr.port(), 8080);
    }

    #[test]
    fn share_link_base_url_override() {
        let mut m = HashMap::new();
        m.insert("SCRIPTUM_RELAY_SHARE_LINK_BASE_URL", "https://app.scriptum.dev/s");
        let cfg = RelayConfig::from_env_fn(env_from_map(m));
        assert_eq!(cfg.share_link_base_url, "https://app.scriptum.dev/s");
    }
}
