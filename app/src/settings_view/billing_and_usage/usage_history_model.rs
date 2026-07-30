use warp_graphql::scalars::Time;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::auth::AuthStateProvider;

const PAGE_SIZE: i32 = 20;

pub struct UsageHistoryModel {
    entries: Vec<warp_graphql::queries::get_conversation_usage::ConversationUsage>,
    is_loading: bool,
    // Whether the server indicated that there may be more entries to load.
    has_more_entries: bool,
}

impl Entity for UsageHistoryModel {
    type Event = ();
}

impl SingletonEntity for UsageHistoryModel {}

impl UsageHistoryModel {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            entries: Vec::new(),
            is_loading: false,
            has_more_entries: true,
        }
    }

    pub fn entries(&self) -> &[warp_graphql::queries::get_conversation_usage::ConversationUsage] {
        &self.entries
    }

    pub fn is_loading(&self) -> bool {
        self.is_loading
    }

    pub fn has_more_entries(&self) -> bool {
        self.has_more_entries
    }

    /// Fetches conversation usage over the past 30 days.
    /// If some usage has already been loaded, this fetches the same number of entries.
    /// If no usage has been loaded, this fetches PAGE_SIZE entries.
    pub fn refresh_usage_history_async(&mut self, ctx: &mut ModelContext<Self>) {
        if self.is_loading || !AuthStateProvider::as_ref(ctx).get().is_logged_in() {
            return;
        }

        // If the user has already loaded some number of entries,
        // we should load that same number of items on refresh so that the list doesn't shrink
        // every time the page is refreshed.
        let num_items_to_fetch = if self.entries.is_empty() {
            PAGE_SIZE
        } else {
            self.entries.len() as i32
        };

        // Reset pagination state and clear any existing entries.
        self.entries.clear();
        self.has_more_entries = true;

        self.fetch_next_page(num_items_to_fetch, None, ctx);
    }

    /// Fetches the next page of conversation usage entries, appending them to the existing list.
    pub fn load_more_usage_history_async(&mut self, ctx: &mut ModelContext<Self>) {
        if self.is_loading || !self.has_more_entries {
            return;
        }

        let last_updated_end_timestamp: Option<Time> =
            self.entries.last().map(|entry| entry.last_updated);
        if last_updated_end_timestamp.is_none() {
            return;
        }

        self.fetch_next_page(PAGE_SIZE, last_updated_end_timestamp, ctx);
    }

    /// LOCAL FORK: conversation usage history was fetched through the AI client on
    /// `ServerApiProvider`, which went with the agent. The model is kept so the
    /// billing pages keep working; it now always reports an empty, fully-loaded
    /// history.
    fn fetch_next_page(
        &mut self,
        _limit: i32,
        _last_updated_end_timestamp: Option<Time>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.is_loading = false;
        self.has_more_entries = false;
        ctx.notify();
    }
}
