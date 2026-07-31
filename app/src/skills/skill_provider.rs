//! Skill provider definitions and utilities.
//!
//! This module defines the supported skill providers (i.e. Agents, Claude, Codex, Warp) and their
//! associated skills directory paths.
//!
//! LOCAL FORK: the path-to-provider lookups (`get_provider_for_path`, `home_skills_path`,
//! `get_scope_for_path`, `provider_rank`, `provider_parent_directory_for_skills_root`) went with
//! the skill parsing they served. What is left is the directory table itself, which `lib.rs`
//! still reads to force-include gitignored skill directories in the file watcher.
use std::path::PathBuf;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString, VariantNames};
use warp_core::ui::color::CLAUDE_ORANGE;
use warp_core::ui::icons::Icon;
use warp_core::ui::theme::Fill;

/// Represents a skill provider/origin (Agents, Claude, Codex, or Warp).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    VariantNames,
)]
pub enum SkillProvider {
    Warp,
    Agents,
    Claude,
    Codex,
    Cursor,
    Gemini,
    Copilot,
    Droid,
    Github,
    OpenCode,
    Kiro,
}

/// Represents the scope of a skill (home directory vs project directory).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Default,
    Display,
    EnumString,
    VariantNames,
)]
pub enum SkillScope {
    /// Skills from the user's home directory (e.g., `~/.agents/skills`).
    #[default]
    Home,
    /// Skills from a project directory (e.g., `./repo/.agents/skills`).
    Project,
    /// Bundled skills distributed with Warp.
    Bundled,
}

/// Definition of a skill provider including its directory path.
///
/// LOCAL FORK: the `provider: SkillProvider` field went with the path-to-provider lookups that
/// were its only readers. The provider each entry belongs to is still readable off the directory
/// name in [`SKILL_PROVIDER_DEFINITIONS`].
pub struct SkillProviderDefinition {
    /// Relative path from root (repo or home), constructed with platform-aware joining.
    pub skills_path: PathBuf,
}

impl SkillProvider {
    /// Returns the default icon for this provider.
    pub fn icon(&self) -> Icon {
        match self {
            SkillProvider::Claude => Icon::ClaudeLogo,
            SkillProvider::Codex => Icon::OpenAILogo,
            SkillProvider::Gemini => Icon::GeminiLogo,
            SkillProvider::Droid => Icon::DroidLogo,
            SkillProvider::OpenCode => Icon::OpenCodeLogo,
            SkillProvider::Warp
            | SkillProvider::Agents
            | SkillProvider::Cursor
            | SkillProvider::Copilot
            | SkillProvider::Github
            | SkillProvider::Kiro => Icon::WarpLogoLight,
        }
    }

    /// Returns the icon fill for this provider, using `fallback` for providers that
    /// don't require a specific color. Claude uses its branded salmon color instead.
    pub fn icon_fill(&self, fallback: Fill) -> Fill {
        match self {
            SkillProvider::Claude => Fill::Solid(CLAUDE_ORANGE),
            _ => fallback,
        }
    }
}

/// All provider definitions. Order determines precedence (first = highest priority).
pub static SKILL_PROVIDER_DEFINITIONS: LazyLock<Vec<SkillProviderDefinition>> =
    LazyLock::new(|| {
        vec![
            // Agents
            SkillProviderDefinition {
                skills_path: PathBuf::from(".agents").join("skills"),
            },
            // Warp
            SkillProviderDefinition {
                skills_path: PathBuf::from(".warp").join("skills"),
            },
            // Claude
            SkillProviderDefinition {
                skills_path: PathBuf::from(".claude").join("skills"),
            },
            // Codex
            SkillProviderDefinition {
                skills_path: PathBuf::from(".codex").join("skills"),
            },
            // Cursor
            SkillProviderDefinition {
                skills_path: PathBuf::from(".cursor").join("skills"),
            },
            // Gemini
            SkillProviderDefinition {
                skills_path: PathBuf::from(".gemini").join("skills"),
            },
            // Copilot
            SkillProviderDefinition {
                skills_path: PathBuf::from(".copilot").join("skills"),
            },
            // Droid
            SkillProviderDefinition {
                skills_path: PathBuf::from(".factory").join("skills"),
            },
            // Github
            SkillProviderDefinition {
                skills_path: PathBuf::from(".github").join("skills"),
            },
            // OpenCode
            SkillProviderDefinition {
                skills_path: PathBuf::from(".opencode").join("skills"),
            },
            // Kiro
            SkillProviderDefinition {
                skills_path: PathBuf::from(".kiro").join("skills"),
            },
        ]
    });
