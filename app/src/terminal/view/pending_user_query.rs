use warpui::ViewContext;

use crate::terminal::TerminalView;

impl TerminalView {
    // LOCAL FORK: fns insert_pending_user_query_block and
    // insert_cloud_mode_queued_user_query_block removed with the agent.

    pub(in crate::terminal::view) fn remove_cloud_mode_queue_row(
        &mut self,
        _ctx: &mut ViewContext<Self>,
    ) {
        // LOCAL FORK: nothing queues prompts anymore, so there is no row to remove.
    }

    /// Removes the pending user query block, if one exists. No-op if none is present.
    /// Also cancels the queued prompt callback so the prompt is not sent.
    /// (Safe to call from within the callback itself — the caller `.take()`s it first.)
    pub(super) fn remove_pending_user_query_block(&mut self, ctx: &mut ViewContext<Self>) {
        self.queued_prompt_callback = None;
        self.pending_user_query_kind = None;
        if let Some(view_id) = self.pending_user_query_view_id.take() {
            self.model
                .lock()
                .block_list_mut()
                .remove_rich_content(view_id);
            self.rich_content_views.retain(|rc| rc.view_id() != view_id);
            ctx.notify();
        }
    }


}
