use fuzzy_match::match_indices_case_insensitive;
use ordered_float::OrderedFloat;
use warpui::{AppContext, Entity, EntityId};

use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::terminal::input::inline_menu::{InlineMenuAction, InlineMenuType};
use crate::terminal::input::profiles::search_item::ProfileSearchItem;

#[derive(Clone, Debug)]
pub enum SelectProfileMenuItem {
    ManageProfiles,
}

impl InlineMenuAction for SelectProfileMenuItem {
    const MENU_TYPE: InlineMenuType = InlineMenuType::ProfileSelector;
}

pub struct ProfileSelectorDataSource {
    terminal_view_id: EntityId,
}

impl ProfileSelectorDataSource {
    pub fn new(terminal_view_id: EntityId) -> Self {
        Self { terminal_view_id }
    }
}

impl SyncDataSource for ProfileSelectorDataSource {
    type Action = SelectProfileMenuItem;

    fn run_query(
        &self,
        query: &Query,
        _app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        let query_text = query.text.trim().to_lowercase();
        let mut results = Vec::new();
        if query_text.is_empty() {
            results.push(QueryResult::from(
                ProfileSearchItem::new_manage_profiles_item(),
            ));
        } else if let Some(match_result) =
            match_indices_case_insensitive("manage profiles", &query_text)
        {
            let score = match_result.score;
            results.push(QueryResult::from(
                ProfileSearchItem::new_manage_profiles_item()
                    .with_match_result(match_result)
                    .with_score(OrderedFloat(score as f64)),
            ));
        }

        // LOCAL FORK: the per-profile entries came from AIExecutionProfilesModel,
        // which went with the agent. Only "Manage profiles" is left.

        Ok(results)
    }
}

impl Entity for ProfileSelectorDataSource {
    type Event = ();
}
