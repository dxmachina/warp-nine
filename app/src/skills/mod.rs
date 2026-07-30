//! Skill discovery and parsing.
//!
//! LOCAL FORK: lifted out of `crates/ai/src/skills/`, minus `conversion.rs`. That
//! one file was the module's only agent dependency (it imported
//! `crate::agent::action_result::{AnyFileContent, FileContext}` to turn a skill
//! into agent tool output) and nothing outside the agent used it. Everything else
//! here is plain filesystem and markdown work: finding skill directories, parsing
//! frontmatter, and naming providers. The app consumes `SKILL_PROVIDER_DEFINITIONS`
//! and `SkillReference`; lifting the module is what lets the `ai` crate go.

mod parse_skill;
mod parser;
mod read_skills;
mod skill_provider;
mod skill_reference;

pub use parse_skill::{
    ParsedSkill, parse_bundled_skill, parse_skill, parse_skill_content_at_location,
};
pub use read_skills::read_skills;
pub use skill_provider::{
    SKILL_PROVIDER_DEFINITIONS, SkillProvider, SkillProviderDefinition, SkillScope,
    get_provider_for_path, home_skills_path, provider_parent_directory_for_skills_root,
    provider_rank,
};
pub use skill_reference::SkillReference;
