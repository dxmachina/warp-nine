// LOCAL FORK: every other item in this module built the AI block / conversation context
// menus and went with the agent. Only the generic menu presenter survives.
use warpui::UpdateView;

use super::{
    CONTEXT_MENU_WIDTH, ContextMenuState, MenuItem, TerminalAction, TerminalView, Tip, TipHint,
    ViewContext, mark_feature_used_and_write_to_user_defaults,
};

impl TerminalView {
    pub(super) fn show_context_menu(
        &mut self,
        menu_state: ContextMenuState,
        items: Vec<MenuItem<TerminalAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        ctx.update_view(&self.context_menu, |context_menu, view_ctx| {
            context_menu.set_origin(menu_state.menu_type.origin());
            context_menu.set_width(CONTEXT_MENU_WIDTH);
            // This will also reset the selection.
            context_menu.set_items(items, view_ctx);
        });

        self.context_menu_state = Some(menu_state);
        ctx.focus(&self.context_menu);
        ctx.notify();

        self.tips_completed.update(ctx, |tips, ctx| {
            mark_feature_used_and_write_to_user_defaults(
                Tip::Hint(TipHint::BlockAction),
                tips,
                ctx,
            );
            ctx.notify();
        });
    }
}
