//! Output format shared by the CLI's global options and `warpctrl`.
//!
//! LOCAL FORK: this enum used to live in the (now removed) `agent` module. It is
//! a plain presentation setting with no agent dependencies, so it was rehoused
//! here rather than deleted; `local_control` depends on it.

use std::fmt;

use clap::ValueEnum;

/// Output format for command results.
#[derive(Debug, Copy, Clone, ValueEnum, Eq, PartialEq, Default)]
pub enum OutputFormat {
    /// Output as JSON.
    #[value(name = "json")]
    Json,
    /// Output as newline-delimited JSON.
    #[value(name = "ndjson")]
    Ndjson,
    /// Output as human-readable text.
    #[default]
    #[value(name = "pretty")]
    Pretty,
    /// Output as plain text.
    #[value(name = "text")]
    Text,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.to_possible_value().expect("no values are skipped");
        f.write_str(value.get_name())
    }
}
