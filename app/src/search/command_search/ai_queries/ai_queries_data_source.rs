use warpui::AppContext;

use crate::search::command_search::searcher::CommandSearchItemAction;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::{DataSourceRunErrorWrapper, SyncDataSource};

/// Manages querying the AI queries in history for Command Search.
pub struct AIQueriesDataSource {}

impl AIQueriesDataSource {
    pub fn new() -> Self {
        Self {}
    }
}

impl SyncDataSource for AIQueriesDataSource {
    type Action = CommandSearchItemAction;

    /// AI query history came out with the agent, so there is nothing to search.
    fn run_query(
        &self,
        _query: &Query,
        _app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        Ok(Vec::new())
    }
}
