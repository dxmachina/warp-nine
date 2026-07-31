//! Skill provider definitions.
//!
//! LOCAL FORK: lifted out of `crates/ai/src/skills/` so the `ai` crate could go;
//! the app consumes `SKILL_PROVIDER_DEFINITIONS` (directory-watcher force-include
//! paths, `lib.rs`) and `SkillReference` (slash commands, editor management), so
//! the module still earns its place.
//!
//! The parsing half is gone: `parse_skill.rs`, `parser.rs` and `read_skills.rs`
//! turned SKILL.md files into `ParsedSkill`s for the agent's `SkillManager`, and
//! with that singleton removed `query_selectable_skills` returns nothing, so
//! nothing called them. Same for the path-to-provider lookups in
//! `skill_provider.rs` (`get_provider_for_path`, `home_skills_path`,
//! `get_scope_for_path`, `provider_rank`, ...), which only ever fed parsing.

mod skill_provider;
mod skill_reference;

// `SkillProviderDefinition` is deliberately not re-exported: consumers only iterate
// `SKILL_PROVIDER_DEFINITIONS` and read `skills_path`, never name the type.
pub use skill_provider::{SKILL_PROVIDER_DEFINITIONS, SkillProvider, SkillScope};
pub use skill_reference::SkillReference;
