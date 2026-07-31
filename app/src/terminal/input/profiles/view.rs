use warpui::elements::ChildView;
use warpui::{Element, Entity, EntityId, ModelHandle, View, ViewContext, ViewHandle};

use crate::search::data_source::Query;
use crate::search::mixer::SearchMixer;
use crate::terminal::input::buffer_model::{InputBufferModel, InputBufferUpdateEvent};
use crate::terminal::input::inline_menu::{InlineMenuEvent, InlineMenuPositioner, InlineMenuView};
use crate::terminal::input::profiles::data_source::{
    ProfileSelectorDataSource, SelectProfileMenuItem,
};
use crate::terminal::input::suggestions_mode_model::{
    InputSuggestionsModeEvent, InputSuggestionsModeModel,
};

#[derive(Debug, Clone)]
pub enum InlineProfileSelectorEvent {
    ManageProfiles,
    Dismissed,
}

pub struct InlineProfileSelectorView {
    menu_view: ViewHandle<InlineMenuView<SelectProfileMenuItem>>,
    mixer: ModelHandle<SearchMixer<SelectProfileMenuItem>>,
    suggestions_mode_model: ModelHandle<InputSuggestionsModeModel>,
    input_buffer_model: ModelHandle<InputBufferModel>,
    terminal_view_id: EntityId,
}

impl InlineProfileSelectorView {
    pub fn new(
        terminal_view_id: EntityId,
        suggestions_mode_model: ModelHandle<InputSuggestionsModeModel>,
        input_buffer_model: &ModelHandle<InputBufferModel>,
        positioner: &ModelHandle<InlineMenuPositioner>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let data_source = ctx.add_model(|_| ProfileSelectorDataSource::new(terminal_view_id));
        let mixer = ctx.add_model(|ctx| {
            let mut mixer = SearchMixer::<SelectProfileMenuItem>::new();
            mixer.add_sync_source(data_source, []);
            mixer.run_query(Query::default(), ctx);
            mixer
        });

        let menu_view = ctx.add_typed_action_view(|ctx| {
            InlineMenuView::new(
                mixer.clone(),
                positioner.clone(),
                &suggestions_mode_model,
                ctx,
            )
        });

        ctx.subscribe_to_view(&menu_view, |_, _, event, ctx| match event {
            InlineMenuEvent::AcceptedItem { item, .. } => match item {
                SelectProfileMenuItem::ManageProfiles => {
                    ctx.emit(InlineProfileSelectorEvent::ManageProfiles);
                }
            },
            InlineMenuEvent::Dismissed => {
                ctx.emit(InlineProfileSelectorEvent::Dismissed);
            }
            InlineMenuEvent::SelectedItem { .. }
            | InlineMenuEvent::NoResults
            | InlineMenuEvent::TabChanged => {}
        });

        ctx.subscribe_to_model(&suggestions_mode_model, |me, model, event, ctx| {
            let InputSuggestionsModeEvent::ModeChanged { .. } = event;
            if model.as_ref(ctx).is_profile_selector() {
                me.mixer.update(ctx, |mixer, ctx| {
                    mixer.run_query(
                        Query {
                            text: me.input_buffer_model.as_ref(ctx).current_value().to_owned(),
                            ..Default::default()
                        },
                        ctx,
                    );
                });
            } else if model.as_ref(ctx).is_closed() {
                me.mixer.update(ctx, |mixer, ctx| {
                    mixer.reset_results(ctx);
                });
            }
        });

        ctx.subscribe_to_model(input_buffer_model, |me, _, event, ctx| {
            if !me.suggestions_mode_model.as_ref(ctx).is_profile_selector() {
                return;
            }

            let InputBufferUpdateEvent {
                new_content,
                old_content,
                ..
            } = event;
            if new_content == old_content {
                // No need to re-run the query if the content hasn't changed.
                return;
            }

            me.mixer.update(ctx, |mixer, ctx| {
                mixer.run_query(
                    Query {
                        text: new_content.clone(),
                        ..Default::default()
                    },
                    ctx,
                );
            });
        });

        // LOCAL FORK: the execution profile model and the active-profile
        // pre-highlight it drove went with the agent. Only "Manage profiles"
        // remains in this menu, so there is nothing left to re-query or select.

        Self {
            menu_view,
            mixer,
            suggestions_mode_model,
            input_buffer_model: input_buffer_model.clone(),
            terminal_view_id,
        }
    }

    pub fn select_up(&self, ctx: &mut ViewContext<Self>) {
        self.menu_view.update(ctx, |view, ctx| view.select_up(ctx));
    }

    pub fn select_down(&self, ctx: &mut ViewContext<Self>) {
        self.menu_view
            .update(ctx, |view, ctx| view.select_down(ctx));
    }

    pub fn accept_selected_item(&self, ctx: &mut ViewContext<Self>) {
        self.menu_view
            .update(ctx, |view, ctx| view.accept_selected_item(false, ctx));
    }
}

impl View for InlineProfileSelectorView {
    fn ui_name() -> &'static str {
        "InlineProfileSelectorView"
    }

    fn render(&self, _app: &warpui::AppContext) -> Box<dyn Element> {
        ChildView::new(&self.menu_view).finish()
    }
}

impl Entity for InlineProfileSelectorView {
    type Event = InlineProfileSelectorEvent;
}
