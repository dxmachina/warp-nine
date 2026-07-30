use warpui::{SingletonEntity as _, ViewContext};

use super::Input;
use crate::appearance::Appearance;
use crate::editor::{EnterSettings, TextColors};

// LOCAL FORK: fn render_cli_agent_input removed with the agent. The CLI agent rich input
// (attachment chips + agent input footer) is gone and the function had no caller left.

impl Input {
    /// Keep the rich input editor's text colors legible when it's rendered on
    /// top of an alt-screen CLI agent's inferred background (e.g. OpenCode),
    /// which does not respect the Warp theme. When no alt-screen-backed CLI
    /// agent rich input is active, restores the theme default text colors.
    pub(super) fn update_cli_agent_editor_text_colors(&mut self, ctx: &mut ViewContext<Self>) {
        let appearance = Appearance::as_ref(ctx);
        // LOCAL FORK: CLI agent sessions went with the agent, so the rich input
        // is never open and the editor always keeps the theme default colors.
        let text_colors = TextColors::from_appearance(appearance);

        self.editor.update(ctx, |editor, ctx| {
            editor.set_text_colors(text_colors, ctx);
        });
    }

    /// Configures the editor's enter-key behaviour for the CLI agent rich input.
    ///
    /// When rich input is **closed**, `EnterSettings::default()` is restored.
    pub(super) fn update_cli_agent_enter_settings(&mut self, ctx: &mut ViewContext<Self>) {
        // LOCAL FORK: CLI agent sessions went with the agent, so the rich input
        // is never open; the editor keeps baseline enter behaviour.
        let settings = EnterSettings::default();

        self.editor.update(ctx, |editor, _ctx| {
            editor.set_enter_settings(settings);
        });
    }
}
