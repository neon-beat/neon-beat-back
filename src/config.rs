//! Application-level configuration loading, including the runtime team colors set.

use std::{env, fs, io::ErrorKind, path::PathBuf};

use serde::Deserialize;
use tracing::{info, warn};

use crate::state::game::TeamColor;

/// Default location on disk where the server looks for the JSON configuration.
const DEFAULT_CONFIG_PATH: &str = "config/app.json";
/// Environment variable that overrides [`DEFAULT_CONFIG_PATH`].
const CONFIG_PATH_ENV: &str = "NEON_BEAT_BACK_CONFIG_PATH";
/// Fallback color returned when the colors set is exhausted.
const DEFAULT_COLOR: TeamColor = TeamColor {
    h: 0.0,
    s: 0.0,
    v: 1.0,
};

#[derive(Debug, Clone)]
/// Immutable runtime configuration shared across the application.
pub struct AppConfig {
    colors: Vec<TeamColor>,
}

impl AppConfig {
    /// Load the application configuration from disk, falling back to a baked-in default colors set.
    pub fn load() -> Self {
        let path = resolve_config_path();
        match fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<RawConfig>(&contents) {
                Ok(raw) => {
                    let app_config: Self = raw.into();
                    info!(
                        path = %path.display(),
                        count = app_config.colors.len(),
                        "loaded team colors set from config"
                    );
                    app_config
                }
                Err(err) => {
                    warn!(
                        path = %path.display(),
                        error = %err,
                        "failed to parse config; falling back to defaults"
                    );
                    Self::default()
                }
            },
            Err(err) if err.kind() == ErrorKind::NotFound => {
                info!(
                    path = %path.display(),
                    "config file not found; using built-in defaults"
                );
                Self::default()
            }
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "failed to read config; falling back to defaults"
                );
                Self::default()
            }
        }
    }

    /// Return the first color for colors set that is not already listed in `used`.
    ///
    /// When every colors set entry is already taken we wrap around to [`TeamColor::default()`] so
    /// callers always receive a value.
    pub fn first_unused_color(&self, used: &[TeamColor]) -> TeamColor {
        self.colors
            .iter()
            .find(|candidate| used.iter().all(|existing| existing != *candidate))
            .cloned()
            .unwrap_or(DEFAULT_COLOR)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

#[derive(Debug, Deserialize)]
/// JSON representation of the configuration file located at [`DEFAULT_CONFIG_PATH`].
struct RawConfig {
    colors: Vec<RawColor>,
}

impl From<RawConfig> for AppConfig {
    fn from(value: RawConfig) -> Self {
        let colors = value.colors.into_iter().map(Into::into).collect::<Vec<_>>();
        Self { colors }
    }
}

#[derive(Debug, Deserialize)]
/// JSON representation of a single HSV entry inside the configuration file.
struct RawColor {
    hue: f32,
    saturation: f32,
    value: f32,
}

impl From<RawColor> for TeamColor {
    fn from(value: RawColor) -> Self {
        Self {
            h: value.hue,
            s: value.saturation,
            v: value.value,
        }
    }
}

/// Resolve the configuration path taking the environment override into account.
fn resolve_config_path() -> PathBuf {
    env::var_os(CONFIG_PATH_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH))
}

/// Built-in colors set shipped with the binary.
fn default_colors() -> Vec<TeamColor> {
    vec![
        TeamColor {
            h: -64.69388,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: 119.331474,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: -113.57562,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: 34.365788,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: -169.41148,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: -19.08323,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: 58.87927,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: -134.34782,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: 153.15997,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: -37.933628,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: -90.79761,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: 44.579124,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: -2.2399259,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: -178.32115,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: -148.47302,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: 12.806246,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: 82.401955,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: 24.292171,
            s: 0.6,
            v: 1.0,
        },
        TeamColor {
            h: 170.61838,
            s: 1.0,
            v: 1.0,
        },
        TeamColor {
            h: -159.7051,
            s: 0.6,
            v: 1.0,
        },
    ]
}
