use fuzzy_match::FuzzyMatchResult;
use ordered_float::OrderedFloat;
use warp_core::ui::appearance::Appearance;
use warpui::keymap::Keystroke;
use warpui::platform::OperatingSystem;
use warpui::{AppContext, Entity, EntityId, SingletonEntity as _, WindowId};

use crate::features::FeatureFlag;
use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::terminal::input::inline_menu::{
    DetailsRenderConfig, InlineMenuAction, InlineMenuMessageArgs, InlineMenuType,
    default_navigation_message_items, styles as inline_styles,
};
use crate::terminal::input::message_bar::{Message, MessageItem};

#[derive(Clone, Debug)]
pub struct AcceptModel {}

impl InlineMenuAction for AcceptModel {
    const MENU_TYPE: InlineMenuType = InlineMenuType::ModelSelector;

    fn produce_inline_menu_message<T>(args: InlineMenuMessageArgs<'_, Self, T>) -> Option<Message> {
        if !FeatureFlag::InlineMenuHeaders.is_enabled() {
            return Some(Message::new(default_navigation_message_items(&args)));
        }

        let mut items = vec![
            MessageItem::keystroke(Keystroke {
                key: "enter".to_owned(),
                ..Default::default()
            }),
            MessageItem::text(" to select"),
            MessageItem::keystroke(if OperatingSystem::get().is_mac() {
                Keystroke {
                    key: "enter".to_owned(),
                    cmd: true,
                    ..Default::default()
                }
            } else {
                Keystroke {
                    key: "enter".to_owned(),
                    ctrl: true,
                    shift: true,
                    ..Default::default()
                }
            }),
            MessageItem::text(" select and save to profile"),
        ];

        if args.inline_menu_model.tab_configs().len() > 1 {
            items.push(MessageItem::keystroke(Keystroke {
                key: "tab".to_owned(),
                shift: true,
                ..Default::default()
            }));
            items.push(MessageItem::text(" to cycle tabs"));
        }

        items.push(MessageItem::clickable(
            vec![
                MessageItem::keystroke(Keystroke {
                    key: "escape".to_owned(),
                    ..Default::default()
                }),
                MessageItem::text(" to dismiss"),
            ],
            |ctx| {
                ctx.dispatch_typed_action(
                    crate::terminal::input::inline_menu::InlineMenuRowAction::<Self>::Dismiss,
                );
            },
            args.inline_menu_model.mouse_states().dismiss.clone(),
        ));

        Some(Message::new(items))
    }

    fn details_render_config(app: &AppContext) -> Option<DetailsRenderConfig> {
        let appearance = Appearance::as_ref(app);
        let max_item_width = app.font_cache().em_width(
            appearance.ui_font_family(),
            inline_styles::font_size(appearance),
        ) * 40.;
        Some(DetailsRenderConfig {
            min_required_details_width: Some(model_specs_width(app)),
            max_result_width: Some(max_item_width),
        })
    }
}

fn model_specs_width(app: &AppContext) -> f32 {
    let appearance = Appearance::as_ref(app);
    app.font_cache().em_width(
        appearance.ui_font_family(),
        appearance.monospace_font_size(),
    ) * 34.
}
/// Frontend-neutral model picker result shared by GUI and TUI surfaces.
#[derive(Clone, Debug)]
pub struct ModelPickerChoice {
    pub name_match_result: Option<FuzzyMatchResult>,
    pub score: OrderedFloat<f64>,
}

impl ModelPickerChoice {
    /// LOCAL FORK: the disable reason came from the agent's LLM catalog.
    pub fn is_selectable(&self) -> bool {
        true
    }
}

/// LOCAL FORK: the choices came from the agent's `LLMPreferences` catalog, so
/// the picker is always empty. Kept so the GUI and TUI surfaces still compile.
pub fn query_model_picker_choices(_query_text: &str, _app: &AppContext) -> Vec<ModelPickerChoice> {
    Vec::new()
}

pub struct ModelSelectorDataSource {
    terminal_view_id: EntityId,
    window_id: WindowId,
}

impl ModelSelectorDataSource {
    pub fn new(terminal_view_id: EntityId, window_id: WindowId) -> Self {
        Self {
            terminal_view_id,
            window_id,
        }
    }

    /// Returns whether a model should appear in the inline picker.
    /// Custom-endpoint models are suppressed in Oz cloud agent panes because
    /// they cannot route through Warp's cloud inference infrastructure.
    pub(crate) fn include_model_in_picker(is_cloud_pane: bool, is_custom_endpoint: bool) -> bool {
        !is_cloud_pane || !is_custom_endpoint
    }

    // LOCAL FORK: fn order_model_choices removed with the agent's LLM catalog.
}

impl SyncDataSource for ModelSelectorDataSource {
    type Action = AcceptModel;

    /// LOCAL FORK: the model list came from the agent's `LLMPreferences`, so the
    /// picker has nothing to offer.
    fn run_query(
        &self,
        _query: &Query,
        _app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        Ok(Vec::new())
    }
}

impl Entity for ModelSelectorDataSource {
    type Event = ();
}

// LOCAL FORK: struct ModelSearchItem and fn should_show_discount_chip removed
// with the agent; they rendered LLM rows for the model picker.
