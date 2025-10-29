//! Application-level configuration loading, including the runtime colors set and buzzer patterns.

use std::{env, fs, io::ErrorKind, path::PathBuf};

use serde::Deserialize;
use tracing::{info, warn};

use crate::{
    dto::{
        common::TeamColorDto,
        ws::{BuzzerPattern, BuzzerPatternDetails},
    },
    state::game::TeamColor,
};

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
/// Fallback color for patterns.
const DEFAULT_COLOR_DTO: TeamColorDto = TeamColorDto {
    h: 0.0,
    s: 0.0,
    v: 1.0,
};

/// Resolve the configuration path taking the environment override into account.
fn resolve_config_path() -> PathBuf {
    env::var_os(CONFIG_PATH_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH))
}

#[derive(Debug, Clone)]
/// Immutable runtime configuration shared across the application.
pub struct AppConfig {
    colors: Vec<TeamColor>,
    patterns: PatternSet,
}

impl AppConfig {
    /// Load the application configuration from disk, falling back to a baked-in default colors set.
    pub fn load() -> Self {
        let path = resolve_config_path();
        match fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<RawConfig>(&contents) {
                Ok(raw) => {
                    let app_config: Self = raw.into();
                    info!(path = %path.display(), "loaded runtime configuration");
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

    /// Return the first color from the colors set that is not already listed in `used`.
    ///
    /// When every colors set entry is already taken we wrap around to `DEFAULT_COLOR` so callers
    /// always receive a value.
    pub fn first_unused_color(&self, used: &[TeamColor]) -> TeamColor {
        self.colors
            .iter()
            .find(|candidate| used.iter().all(|existing| existing != *candidate))
            .cloned()
            .unwrap_or(DEFAULT_COLOR)
    }

    /// Retrieve the buzzer pattern preset for the requested state.
    ///
    /// For presets carrying a `TeamColorDto`, that color is used unless the configuration specifies
    /// a `static_color`, allowing administrators to override the colors set on a per-pattern basis.
    pub fn buzzer_pattern(&self, preset: BuzzerPatternPreset) -> BuzzerPattern {
        self.patterns.pattern(preset)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
            patterns: default_patterns(),
        }
    }
}

#[derive(Debug, Deserialize)]
/// JSON representation of the configuration file located at [`DEFAULT_CONFIG_PATH`].
struct RawConfig {
    #[serde(default)]
    colors: Vec<RawColor>,
    #[serde(default)]
    patterns: Option<RawPatternSet>,
}

impl From<RawConfig> for AppConfig {
    fn from(value: RawConfig) -> Self {
        let colors = if value.colors.is_empty() {
            default_colors()
        } else {
            value.colors.into_iter().map(Into::into).collect::<Vec<_>>()
        };
        let patterns = value
            .patterns
            .map(override_default_patterns)
            .unwrap_or_else(default_patterns);
        Self { colors, patterns }
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

#[derive(Debug, Deserialize)]
/// JSON representation of buzzer patterns.
struct RawPatternSet {
    #[serde(default)]
    waiting_for_pairing: Option<RawPatternTemplate>,
    #[serde(default)]
    standby: Option<RawPatternTemplate>,
    #[serde(default)]
    playing: Option<RawPatternTemplate>,
    #[serde(default)]
    answering: Option<RawPatternTemplate>,
    #[serde(default)]
    waiting: Option<RawPatternTemplate>,
}

impl RawPatternSet {
    /// Overlay user-provided pattern templates onto the default set.
    fn merge(self, mut defaults: PatternSet) -> PatternSet {
        if let Some(pattern) = self.waiting_for_pairing {
            defaults.waiting_for_pairing = pattern.into_template(&defaults.waiting_for_pairing);
        }
        if let Some(pattern) = self.standby {
            defaults.standby = pattern.into_template(&defaults.standby);
        }
        if let Some(pattern) = self.playing {
            defaults.playing = pattern.into_template(&defaults.playing);
        }
        if let Some(pattern) = self.answering {
            defaults.answering = pattern.into_template(&defaults.answering);
        }
        if let Some(pattern) = self.waiting {
            defaults.waiting = pattern.into_template(&defaults.waiting);
        }
        defaults
    }
}

/// Convenience helper to merge raw patterns onto the defaults.
fn override_default_patterns(raw_pattern_set: RawPatternSet) -> PatternSet {
    raw_pattern_set.merge(default_patterns())
}

#[derive(Debug, Deserialize)]
/// User-supplied pattern template entry from configuration.
struct RawPatternTemplate {
    #[serde(flatten)]
    kind: RawPatternKind,
    #[serde(default)]
    static_color: Option<RawColor>,
}

impl RawPatternTemplate {
    /// Convert the raw template into an internal [`PatternTemplate`], inheriting values from
    /// `default` when they are not supplied in the configuration.
    fn into_template(self, default: &PatternTemplate) -> PatternTemplate {
        let static_color = self
            .static_color
            .map(TeamColor::from)
            .map(Into::into)
            .or(default.static_color);

        PatternTemplate {
            kind: self.kind,
            static_color,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// Raw discriminated union describing the LED pattern kind.
enum RawPatternKind {
    Blink(RawPatternDetails),
    Wave(RawPatternDetails),
    Off,
}

#[derive(Clone, Debug, Deserialize)]
/// Timing metadata attached to blink/wave patterns.
struct RawPatternDetails {
    duration_ms: usize,
    period_ms: usize,
    dc: f32,
}

impl RawPatternDetails {
    fn to_buzzer_pattern_details(&self, color: TeamColorDto) -> BuzzerPatternDetails {
        BuzzerPatternDetails {
            duration_ms: self.duration_ms,
            period_ms: self.period_ms,
            dc: self.dc,
            color,
        }
    }
}

#[derive(Debug, Clone)]
/// Internal representation of a pattern preset, optionally carrying a static color.
struct PatternTemplate {
    kind: RawPatternKind,
    static_color: Option<TeamColorDto>,
}

impl PatternTemplate {
    /// Construct a blink template.
    fn blink(
        duration_ms: usize,
        period_ms: usize,
        dc: f32,
        static_color: Option<TeamColorDto>,
    ) -> Self {
        Self {
            kind: RawPatternKind::Blink(RawPatternDetails {
                duration_ms,
                period_ms,
                dc,
            }),
            static_color,
        }
    }

    /// Construct a wave template.
    fn wave(
        duration_ms: usize,
        period_ms: usize,
        dc: f32,
        static_color: Option<TeamColorDto>,
    ) -> Self {
        Self {
            kind: RawPatternKind::Wave(RawPatternDetails {
                duration_ms,
                period_ms,
                dc,
            }),
            static_color,
        }
    }

    /// Construct a template that fully disables LEDs.
    fn off() -> Self {
        Self {
            kind: RawPatternKind::Off,
            static_color: None,
        }
    }

    /// Materialise a [`BuzzerPattern`] from the template, using `fallback` when no static color is
    /// configured.
    fn pattern(&self, fallback: Option<TeamColor>) -> BuzzerPattern {
        match &self.kind {
            RawPatternKind::Off => BuzzerPattern::Off,
            RawPatternKind::Blink(details) => BuzzerPattern::Blink(
                details.to_buzzer_pattern_details(self.resolve_color(fallback)),
            ),
            RawPatternKind::Wave(details) => {
                BuzzerPattern::Wave(details.to_buzzer_pattern_details(self.resolve_color(fallback)))
            }
        }
    }

    fn resolve_color(&self, fallback: Option<TeamColor>) -> TeamColorDto {
        self.static_color
            .or(fallback.map(Into::into))
            .unwrap_or(DEFAULT_COLOR_DTO)
    }
}

/// Collection of buzzer pattern templates for different game states.
#[derive(Debug, Clone)]
pub struct PatternSet {
    /// Pattern used while waiting for pairing during prep; uses the provided team color if any.
    waiting_for_pairing: PatternTemplate,
    /// Pattern shown during standby (no active question).
    standby: PatternTemplate,
    /// Pattern used when a team is actively allowed to answer.
    playing: PatternTemplate,
    /// Pattern used for the team currently answering.
    answering: PatternTemplate,
    /// Pattern applied to teams that are temporarily waiting.
    waiting: PatternTemplate,
}

impl PatternSet {
    /// Obtain a concrete buzzer pattern for the requested preset.
    pub fn pattern(&self, preset: BuzzerPatternPreset) -> BuzzerPattern {
        match preset {
            BuzzerPatternPreset::WaitingForPairing => self.waiting_for_pairing.pattern(None),
            BuzzerPatternPreset::Standby(color) => self.standby.pattern(Some(color)),
            BuzzerPatternPreset::Playing(color) => self.playing.pattern(Some(color)),
            BuzzerPatternPreset::Answering(color) => self.answering.pattern(Some(color)),
            BuzzerPatternPreset::Waiting => self.waiting.pattern(None),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
/// Identifiers for the predefined buzzer patterns.
pub enum BuzzerPatternPreset {
    /// Pattern used during prep pairing; color comes from the target team.
    WaitingForPairing,
    /// Pattern displayed while a team is idle/standing by.
    Standby(TeamColor),
    /// Pattern indicating a team is currently allowed to answer.
    Playing(TeamColor),
    /// Pattern used when a team is actively answering.
    Answering(TeamColor),
    /// Pattern for teams temporarily waiting (no color information required).
    Waiting,
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

fn default_patterns() -> PatternSet {
    PatternSet {
        waiting_for_pairing: PatternTemplate::blink(
            1_000,
            200,
            0.5,
            Some(TeamColorDto {
                h: 125.0,
                s: 1.0,
                v: 1.0,
            }), // green
        ),
        standby: PatternTemplate::wave(0, 5_000, 0.2, None),
        playing: PatternTemplate::wave(0, 3_000, 0.5, None),
        answering: PatternTemplate::blink(0, 500, 0.5, None),
        waiting: PatternTemplate::off(),
    }
}
