use std::{
    env,
    ffi::OsString,
    fs,
    path::PathBuf,
    time::Duration,
};

use serde::Deserialize;

const DEFAULT_REFRESH_INTERVAL_MS: u64 = 1000;
const MINIMUM_REFRESH_INTERVAL_MS: u64 = 100;
const MAXIMUM_REFRESH_INTERVAL_MS: u64 = 60_000;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub refresh_interval_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: DEFAULT_REFRESH_INTERVAL_MS,
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not read user configuration");
                return Self::default();
            }
        };
        match toml::from_str(&contents) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "invalid user configuration");
                Self::default()
            }
        }
    }

    pub fn refresh_interval(self) -> Duration {
        Duration::from_millis(
            self.refresh_interval_ms
                .clamp(MINIMUM_REFRESH_INTERVAL_MS, MAXIMUM_REFRESH_INTERVAL_MS),
        )
    }
}

fn config_path() -> Option<PathBuf> {
    config_path_from(
        env::var_os("XDG_CONFIG_HOME"),
        env::var_os("HOME"),
    )
}

fn config_path_from(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let root = xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(|home| PathBuf::from(home).join(".config"))
                .filter(|path| path.is_absolute())
        })?;
    Some(root.join("gamemon").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_one_second() {
        assert_eq!(AppConfig::default().refresh_interval(), Duration::from_secs(1));
    }

    #[test]
    fn parses_user_refresh_interval() {
        let config: AppConfig = toml::from_str("refresh_interval_ms = 2500\n")
            .expect("valid configuration should parse");
        assert_eq!(config.refresh_interval(), Duration::from_millis(2500));
    }

    #[test]
    fn clamps_out_of_range_refresh_intervals() {
        let too_fast: AppConfig = toml::from_str("refresh_interval_ms = 1\n").unwrap();
        let too_slow: AppConfig = toml::from_str("refresh_interval_ms = 999999\n").unwrap();
        assert_eq!(too_fast.refresh_interval(), Duration::from_millis(100));
        assert_eq!(too_slow.refresh_interval(), Duration::from_secs(60));
    }

    #[test]
    fn defaults_missing_configuration_fields() {
        let config: AppConfig = toml::from_str("# future user preferences\n").unwrap();
        assert_eq!(config.refresh_interval(), Duration::from_secs(1));
    }

    #[test]
    fn prefers_absolute_xdg_path_and_falls_back_to_home() {
        let path = config_path_from(
            Some("/tmp/gamemon-config".into()),
            Some("/home/example".into()),
        );
        assert_eq!(
            path,
            Some(PathBuf::from("/tmp/gamemon-config/gamemon/config.toml"))
        );

        let path = config_path_from(Some("relative".into()), Some("/home/example".into()));
        assert_eq!(
            path,
            Some(PathBuf::from("/home/example/.config/gamemon/config.toml"))
        );
        assert_eq!(config_path_from(None, None), None);
    }
}
