//! Data source for the rewind menu.

use warpui::{AppContext, Entity};

use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::terminal::input::rewind::search_item::RewindSearchItem;

/// Action emitted when a rewind point is selected.
#[derive(Clone, Debug)]
pub struct SelectRewindPoint {}

/// Information about file changes for a rewind point.
#[derive(Debug, Clone, Default)]
pub struct FileChangesInfo {
    pub lines_added: usize,
    pub lines_removed: usize,
}

pub struct RewindDataSource {}

impl RewindDataSource {
    pub fn new() -> Self {
        Self {}
    }

    // LOCAL FORK: fn get_file_changes_for_block removed with the agent.
}

impl SyncDataSource for RewindDataSource {
    type Action = SelectRewindPoint;

    fn run_query(
        &self,
        _query: &Query,
        _app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        // LOCAL FORK: rewind points came from agent conversation exchanges, so the only
        // entry left is "Current".
        Ok(vec![QueryResult::from(RewindSearchItem::new_current())])
    }
}

impl Entity for RewindDataSource {
    type Event = ();
}
