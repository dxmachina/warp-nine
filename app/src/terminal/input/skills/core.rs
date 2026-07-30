use ai::skills::{SkillProvider, SkillReference, SkillScope};
use fuzzy_match::FuzzyMatchResult;
use ordered_float::OrderedFloat;
use warp_core::ui::icons::Icon;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{AppContext, EntityId};

pub const LOCAL_SKILLS_REMOTE_EXECUTION_ERROR_MESSAGE: &str = "Local skills cannot run on a remote machine. Try forking the conversation locally and running the skill.";

/// Surface-neutral skill selection result shared by GUI and TUI menus.
#[derive(Clone)]
pub struct SelectableSkill {
    pub name: String,
    pub reference: SkillReference,
    pub description: String,
    pub scope: SkillScope,
    pub provider: SkillProvider,
    pub icon_override: Option<Icon>,
    pub name_match_result: Option<FuzzyMatchResult>,
    pub score: OrderedFloat<f64>,
}

/// Returns skills available for selection in the active input surface.
///
/// LOCAL FORK: discovery, CLI-agent provider filtering and the bundled-skill
/// policy all lived on the agent's `SkillManager` singleton, which is gone. The
/// signature is kept so both frontend adapters still compile; there is nothing
/// left to offer.
pub fn query_selectable_skills(
    _working_directory: Option<&LocalOrRemotePath>,
    _terminal_view_id: EntityId,
    _include_bundled: bool,
    _query_text: &str,
    _app: &AppContext,
) -> Vec<SelectableSkill> {
    Vec::new()
}
