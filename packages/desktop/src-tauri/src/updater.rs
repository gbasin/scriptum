use semver::Version;
use serde::{Deserialize, Serialize};
use std::{env, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;
use url::Url;

const UPDATER_AVAILABLE_EVENT: &str = "scriptum://updater/available";
const UPDATER_SKIPPED_EVENT: &str = "scriptum://updater/skipped";
const UPDATER_INSTALLED_EVENT: &str = "scriptum://updater/installed";

const ENV_UPDATER_ENABLED: &str = "SCRIPTUM_UPDATER_ENABLED";
const ENV_UPDATER_CHECK_ON_STARTUP: &str = "SCRIPTUM_UPDATER_CHECK_ON_STARTUP";
const ENV_UPDATER_RING: &str = "SCRIPTUM_UPDATER_RING";
const ENV_UPDATER_KILL_SWITCH: &str = "SCRIPTUM_UPDATER_KILL_SWITCH";
const ENV_UPDATER_CRASH_RATE_BASELINE: &str = "SCRIPTUM_UPDATER_CRASH_RATE_BASELINE";
const ENV_UPDATER_CRASH_RATE_CURRENT: &str = "SCRIPTUM_UPDATER_CRASH_RATE_CURRENT";
const ENV_UPDATER_ENDPOINTS: &str = "SCRIPTUM_UPDATER_ENDPOINTS";
const ENV_UPDATER_PUBKEY: &str = "SCRIPTUM_UPDATER_PUBKEY";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateRing {
    Internal,
    Beta,
    Ga,
}

impl UpdateRing {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "internal" => Some(Self::Internal),
            "beta" => Some(Self::Beta),
            "ga" | "stable" => Some(Self::Ga),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct UpdaterRuntimeConfig {
    enabled: bool,
    check_on_startup: bool,
    ring: UpdateRing,
    kill_switch_active: bool,
    kill_switch_reason: Option<String>,
    endpoints: Vec<Url>,
    pubkey: Option<String>,
}

impl UpdaterRuntimeConfig {
    fn from_env() -> Self {
        let enabled = read_bool_env(ENV_UPDATER_ENABLED, true);
        let check_on_startup = read_bool_env(ENV_UPDATER_CHECK_ON_STARTUP, true);
        let ring = env::var(ENV_UPDATER_RING)
            .ok()
            .as_deref()
            .and_then(UpdateRing::parse)
            .unwrap_or(UpdateRing::Ga);
        let kill_switch_override = read_bool_env(ENV_UPDATER_KILL_SWITCH, false);
        let baseline = read_f64_env(ENV_UPDATER_CRASH_RATE_BASELINE);
        let current = read_f64_env(ENV_UPDATER_CRASH_RATE_CURRENT);
        let crash_regression = is_crash_regression(baseline, current);
        let kill_switch_active = kill_switch_override || crash_regression;
        let kill_switch_reason = if kill_switch_override {
            Some("Updater kill-switch is explicitly enabled".to_string())
        } else if crash_regression {
            Some("Updater kill-switch activated: crash rate is over 2x baseline".to_string())
        } else {
            None
        };

        let endpoints =
            env::var(ENV_UPDATER_ENDPOINTS).ok().map(parse_endpoints).unwrap_or_default();
        let pubkey = env::var(ENV_UPDATER_PUBKEY)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Self {
            enabled,
            check_on_startup,
            ring,
            kill_switch_active,
            kill_switch_reason,
            endpoints,
            pubkey,
        }
    }

    fn blocked_reason(&self) -> Option<String> {
        if !self.enabled {
            return Some("Updater is disabled".to_string());
        }
        if self.kill_switch_active {
            return self.kill_switch_reason.clone();
        }
        if self.endpoints.is_empty() {
            return Some(
                "No updater endpoints configured (set SCRIPTUM_UPDATER_ENDPOINTS)".to_string(),
            );
        }
        if self.pubkey.is_none() {
            return Some("Updater public key is missing (set SCRIPTUM_UPDATER_PUBKEY)".to_string());
        }
        None
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterPolicySnapshot {
    pub enabled: bool,
    pub check_on_startup: bool,
    pub ring: UpdateRing,
    pub kill_switch_active: bool,
    pub blocked_reason: Option<String>,
    pub endpoint_count: usize,
    pub has_pubkey: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterCheckResult {
    pub ring: UpdateRing,
    pub checked: bool,
    pub available: bool,
    pub blocked_reason: Option<String>,
    pub error: Option<String>,
    pub current_version: Option<String>,
    pub version: Option<String>,
    pub body: Option<String>,
    pub published_at: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterInstallResult {
    pub ring: UpdateRing,
    pub installed: bool,
    pub blocked_reason: Option<String>,
    pub error: Option<String>,
    pub version: Option<String>,
    pub restart_requested: bool,
}

struct UpdaterController {
    config: UpdaterRuntimeConfig,
    last_check: Mutex<Option<UpdaterCheckResult>>,
}

pub fn init<R: Runtime>(app: &AppHandle<R>) {
    if app.try_state::<UpdaterController>().is_some() {
        return;
    }

    let config = UpdaterRuntimeConfig::from_env();
    app.manage(UpdaterController { config: config.clone(), last_check: Mutex::new(None) });

    if config.check_on_startup {
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let result = check_for_updates(app_handle.clone(), false).await;
            if result.available {
                let _ = app_handle.emit(UPDATER_AVAILABLE_EVENT, &result);
            } else if result.blocked_reason.is_some() || result.error.is_some() {
                let _ = app_handle.emit(UPDATER_SKIPPED_EVENT, &result);
            }
        });
    }
}

pub fn policy_snapshot<R: Runtime>(app: &AppHandle<R>) -> UpdaterPolicySnapshot {
    let config = app
        .try_state::<UpdaterController>()
        .map(|state| state.config.clone())
        .unwrap_or_else(UpdaterRuntimeConfig::from_env);
    UpdaterPolicySnapshot {
        enabled: config.enabled,
        check_on_startup: config.check_on_startup,
        ring: config.ring,
        kill_switch_active: config.kill_switch_active,
        blocked_reason: config.blocked_reason(),
        endpoint_count: config.endpoints.len(),
        has_pubkey: config.pubkey.is_some(),
    }
}

pub async fn check_for_updates<R: Runtime>(
    app: AppHandle<R>,
    _user_initiated: bool,
) -> UpdaterCheckResult {
    let config = app
        .try_state::<UpdaterController>()
        .map(|state| state.config.clone())
        .unwrap_or_else(UpdaterRuntimeConfig::from_env);
    let mut result = UpdaterCheckResult {
        ring: config.ring,
        checked: false,
        available: false,
        blocked_reason: config.blocked_reason(),
        error: None,
        current_version: None,
        version: None,
        body: None,
        published_at: None,
        target: None,
    };

    if result.blocked_reason.is_some() {
        update_last_check(&app, result.clone());
        return result;
    }

    let Some(pubkey) = config.pubkey.clone() else {
        result.blocked_reason = Some("Updater public key is missing".to_string());
        update_last_check(&app, result.clone());
        return result;
    };

    let ring = config.ring;
    let mut builder = app.updater_builder().pubkey(pubkey).version_comparator(
        move |current_version, remote_release| {
            remote_release.version > current_version
                && ring_allows_version(ring, &remote_release.version)
        },
    );

    builder = match builder.endpoints(config.endpoints.clone()) {
        Ok(builder) => builder,
        Err(error) => {
            result.error = Some(error.to_string());
            update_last_check(&app, result.clone());
            return result;
        }
    };

    let updater = match builder.build() {
        Ok(updater) => updater,
        Err(error) => {
            result.error = Some(error.to_string());
            update_last_check(&app, result.clone());
            return result;
        }
    };

    result.checked = true;
    match updater.check().await {
        Ok(Some(update)) => {
            result.available = true;
            result.current_version = Some(update.current_version);
            result.version = Some(update.version);
            result.body = update.body;
            result.published_at = update.date.map(|date| date.to_string());
            result.target = Some(update.target);
        }
        Ok(None) => {}
        Err(error) => {
            result.error = Some(error.to_string());
        }
    }

    update_last_check(&app, result.clone());
    result
}

pub async fn install_update<R: Runtime>(app: AppHandle<R>) -> UpdaterInstallResult {
    let config = app
        .try_state::<UpdaterController>()
        .map(|state| state.config.clone())
        .unwrap_or_else(UpdaterRuntimeConfig::from_env);
    let mut result = UpdaterInstallResult {
        ring: config.ring,
        installed: false,
        blocked_reason: config.blocked_reason(),
        error: None,
        version: None,
        restart_requested: false,
    };

    if result.blocked_reason.is_some() {
        return result;
    }

    let Some(pubkey) = config.pubkey.clone() else {
        result.blocked_reason = Some("Updater public key is missing".to_string());
        return result;
    };

    let ring = config.ring;
    let mut builder = app.updater_builder().pubkey(pubkey).version_comparator(
        move |current_version, remote_release| {
            remote_release.version > current_version
                && ring_allows_version(ring, &remote_release.version)
        },
    );

    builder = match builder.endpoints(config.endpoints.clone()) {
        Ok(builder) => builder,
        Err(error) => {
            result.error = Some(error.to_string());
            return result;
        }
    };

    let updater = match builder.build() {
        Ok(updater) => updater,
        Err(error) => {
            result.error = Some(error.to_string());
            return result;
        }
    };

    let Some(update) = (match updater.check().await {
        Ok(update) => update,
        Err(error) => {
            result.error = Some(error.to_string());
            return result;
        }
    }) else {
        result.blocked_reason = Some("No update is currently available".to_string());
        return result;
    };

    let version = update.version.clone();
    if let Err(error) = update.download_and_install(|_, _| {}, || {}).await {
        result.error = Some(error.to_string());
        return result;
    }

    result.installed = true;
    result.version = Some(version);
    result.restart_requested = true;
    let _ = app.emit(UPDATER_INSTALLED_EVENT, &result);
    app.request_restart();
    result
}

pub fn last_check<R: Runtime>(app: &AppHandle<R>) -> Option<UpdaterCheckResult> {
    app.try_state::<UpdaterController>()
        .and_then(|state| state.last_check.lock().ok().and_then(|result| (*result).clone()))
}

fn update_last_check<R: Runtime>(app: &AppHandle<R>, result: UpdaterCheckResult) {
    if let Some(state) = app.try_state::<UpdaterController>() {
        if let Ok(mut guard) = state.last_check.lock() {
            *guard = Some(result);
        }
    }
}

fn ring_allows_version(ring: UpdateRing, version: &Version) -> bool {
    if version.pre.is_empty() {
        return true;
    }

    let prerelease = version.pre.to_string().to_ascii_lowercase();
    match ring {
        UpdateRing::Internal => true,
        UpdateRing::Beta => !prerelease.contains("internal"),
        UpdateRing::Ga => false,
    }
}

fn parse_endpoints(raw: String) -> Vec<Url> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| Url::parse(value).ok())
        .collect()
}

fn read_bool_env(key: &str, default: bool) -> bool {
    env::var(key).ok().as_deref().and_then(parse_bool).unwrap_or(default)
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn read_f64_env(key: &str) -> Option<f64> {
    env::var(key).ok()?.trim().parse().ok()
}

fn is_crash_regression(baseline: Option<f64>, current: Option<f64>) -> bool {
    let Some(baseline) = baseline else {
        return false;
    };
    let Some(current) = current else {
        return false;
    };
    if baseline <= 0.0 || current < 0.0 {
        return false;
    }
    current > (baseline * 2.0)
}

#[cfg(test)]
mod tests {
    use super::{is_crash_regression, parse_bool, ring_allows_version, UpdateRing};
    use semver::Version;

    #[test]
    fn parse_bool_accepts_common_values() {
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("yes"), Some(true));
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("off"), Some(false));
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn ring_policy_filters_prereleases() {
        let stable = Version::parse("1.4.0").unwrap();
        let beta = Version::parse("1.5.0-beta.1").unwrap();
        let internal = Version::parse("1.5.0-internal.2").unwrap();

        assert!(ring_allows_version(UpdateRing::Ga, &stable));
        assert!(!ring_allows_version(UpdateRing::Ga, &beta));
        assert!(ring_allows_version(UpdateRing::Beta, &beta));
        assert!(!ring_allows_version(UpdateRing::Beta, &internal));
        assert!(ring_allows_version(UpdateRing::Internal, &internal));
    }

    #[test]
    fn crash_regression_triggers_at_more_than_twice_baseline() {
        assert!(!is_crash_regression(Some(0.2), Some(0.39)));
        assert!(!is_crash_regression(Some(0.2), Some(0.4)));
        assert!(is_crash_regression(Some(0.2), Some(0.41)));
        assert!(!is_crash_regression(None, Some(0.41)));
    }
}
