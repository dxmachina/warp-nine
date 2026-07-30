//! The agent execution harness identifier.
//!
//! LOCAL FORK: this type used to live in the (now removed) `agent` module. The
//! agent subcommands are gone, but `app` and `cloud_object_models` still name
//! harnesses, so the type was rehoused here rather than deleted.

use std::fmt;

use clap::ValueEnum;
use clap::builder::PossibleValue;
use serde::{Deserialize, Serialize};

const HARNESS_VALUE_VARIANTS: [Harness; 5] = [
    Harness::Oz,
    Harness::Claude,
    Harness::OpenCode,
    Harness::Gemini,
    Harness::Codex,
];

/// The execution harness for an agent run.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    /// Use Warp's built-in MAA infrastructure (default).
    #[default]
    Oz,
    /// Delegate to the `claude` CLI.
    Claude,
    /// Delegate to the `opencode` CLI.
    OpenCode,
    /// Delegate to the `gemini` CLI.
    Gemini,
    /// Delegate to the `codex` CLI.
    Codex,
    /// A harness produced by a newer client/server that this client doesn't
    /// recognize. Surfaced via deserialization fallbacks (e.g. unknown GraphQL
    /// enum values, unknown `harness_type` strings); never selectable from the
    /// CLI or harness dropdown.
    #[serde(other)]
    Unknown,
}

impl ValueEnum for Harness {
    fn value_variants<'a>() -> &'a [Self] {
        &HARNESS_VALUE_VARIANTS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        let mut pv = match self {
            Harness::Oz => {
                PossibleValue::new("oz").help("Use Warp's built-in MAA infrastructure (default)")
            }
            Harness::Claude => PossibleValue::new("claude")
                .alias("claude-code")
                .help("Delegate to the `claude` CLI"),
            Harness::OpenCode => PossibleValue::new("opencode")
                .alias("open-code")
                .help("Delegate to the `opencode` CLI"),
            Harness::Gemini => PossibleValue::new("gemini").help("Delegate to the `gemini` CLI"),
            Harness::Codex => PossibleValue::new("codex").help("Delegate to the `codex` CLI"),
            Harness::Unknown => return None,
        };
        if !self.should_display_in_help_text() {
            pv = pv.hide(true);
        }
        Some(pv)
    }
}

impl Harness {
    pub fn parse_orchestration_harness(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
        <Self as ValueEnum>::from_str(&normalized, true).ok()
    }

    pub fn parse_local_child_harness(value: &str) -> Option<Self> {
        match Self::parse_orchestration_harness(value) {
            Some(harness @ (Self::Claude | Self::OpenCode | Self::Codex)) => Some(harness),
            Some(Self::Oz) | Some(Self::Gemini) | Some(Self::Unknown) | None => None,
        }
    }

    /// Whether this harness is surfaced to users when the value enum is rendered
    /// into help text. Only the generally available harnesses are shown; gemini
    /// and opencode aren't available yet, so they're hidden. Update this when a
    /// harness becomes generally available.
    ///
    /// This is the single source of truth for the `ValueEnum` help text; the
    /// per-variant `#[value(hide = ...)]` attributes are no longer used. It does
    /// not affect runtime acceptance.
    pub fn should_display_in_help_text(self) -> bool {
        match self {
            Self::Oz | Self::Claude | Self::Codex => true,
            Self::OpenCode | Self::Gemini | Self::Unknown => false,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Oz => "Oz",
            Self::Claude => "Claude Code",
            Self::OpenCode => "OpenCode",
            Self::Gemini => "Gemini CLI",
            Self::Codex => "Codex",
            Self::Unknown => "Unknown",
        }
    }

    /// Parses a harness config-name string (the lowercase name written into
    /// `HarnessConfig::harness_type` by the spawner, e.g. `"claude"`, `"gemini"`, `"oz"`)
    /// into a [`Harness`] variant. Inverse of [`Harness::config_name`]. Returns `None` for
    /// unrecognized names so callers can distinguish a future-server harness from a
    /// round-tripped [`Harness::Unknown`]; callers that want to fall back to `Unknown`
    /// should `.unwrap_or(Harness::Unknown)`. UI surfaces should treat `Unknown` as a
    /// non-Oz, non-runnable harness.
    pub fn from_config_name(name: &str) -> Option<Self> {
        match name {
            "oz" => Some(Harness::Oz),
            "claude" => Some(Harness::Claude),
            "opencode" => Some(Harness::OpenCode),
            "gemini" => Some(Harness::Gemini),
            "codex" => Some(Harness::Codex),
            "unknown" => Some(Harness::Unknown),
            _ => None,
        }
    }

    /// Canonical config name for this harness (the lowercase string written into
    /// `HarnessConfig::harness_type`). Inverse of [`Harness::from_config_name`].
    /// The exhaustive match here forces every new [`Harness`] variant to declare a
    /// canonical name, which prevents `from_config_name` from silently falling back to
    /// `Unknown` when a new variant is added.
    pub fn config_name(self) -> &'static str {
        match self {
            Harness::Oz => "oz",
            Harness::Claude => "claude",
            Harness::OpenCode => "opencode",
            Harness::Gemini => "gemini",
            Harness::Codex => "codex",
            Harness::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Harness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.config_name())
    }
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;
