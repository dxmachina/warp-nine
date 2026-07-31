use futures::FutureExt;
use warpui::{App, SingletonEntity};

use super::{AISettings, FEATURE_INTROS, FeatureIntroId, OneTimeModalModel};
use crate::test_util::terminal::{add_window_with_terminal, initialize_app_for_terminal_view};

#[test]
fn wait_until_auto_handoff_sleep_modal_closed_tracks_modal_state() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |_, ctx| {
            OneTimeModalModel::handle(ctx).update(ctx, |model, ctx| {
                // Resolves immediately while the modal is closed.
                assert!(
                    model
                        .wait_until_auto_handoff_sleep_modal_closed()
                        .now_or_never()
                        .is_some()
                );

                // The auto-resume path creates its wait future before the
                // modal opens (e.g. while offline during sleep); it must
                // still observe the modal that opens later.
                let pending_probe = model.wait_until_auto_handoff_sleep_modal_closed();
                let resolving_waiter = model.wait_until_auto_handoff_sleep_modal_closed();

                model.set_auto_handoff_sleep_modal_open(true, ctx);

                // Pending while the modal is open, because the future reads
                // live modal state at poll time.
                assert!(pending_probe.now_or_never().is_none());

                model.mark_auto_handoff_sleep_modal_dismissed(ctx);

                // An existing waiter resolves once the modal closes.
                assert!(resolving_waiter.now_or_never().is_some());
            });
        });
    });
}

// LOCAL FORK: `test_free_ai_removal_modal_decision_matrix` went with the free-AI-removal
// modal. It exercised the decision table over customer type, BYOK/BYOE and credit balance.

// LOCAL FORK: this was `feature_intro_triggers_for_unseen_feature`, and it asserted
// that triggering marks the feature seen up front "whether or not it is shown on the
// current channel". That was right while the registry had entries. Every registered
// intro announced an agent feature, so `FEATURE_INTROS` is now `&[]`
// (`view/feature_intro_modal/view.rs:75`) and `check_and_trigger_feature_intro_modal`
// returns at its `let Some(id) = next_id else` without marking anything.
//
// The machinery is intact and `FeatureIntroId` still has variants, so rather than
// delete the coverage this now pins the empty-registry contract: triggering is a
// no-op, not a panic and not a spurious modal. If an intro is ever registered again,
// restore the original assertions from `git show main:` on this file.
#[test]
fn feature_intro_trigger_is_a_no_op_with_empty_registry() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |_, ctx| {
            let key = FeatureIntroId::CustomModelRouter.as_key();

            OneTimeModalModel::handle(ctx).update(ctx, |model, ctx| {
                assert!(!AISettings::as_ref(ctx).is_feature_intro_seen(key));
                // Simulate the startup race where the modal queue runs before
                // on_active_window_changed has assigned a target window.
                model.target_window_id = None;

                assert!(!model.check_and_trigger_feature_intro_modal(ctx));
                assert!(!AISettings::as_ref(ctx).is_feature_intro_seen(key));
                assert_eq!(model.active_feature_intro, None);

                // Still a no-op on a second call.
                assert!(!model.check_and_trigger_feature_intro_modal(ctx));
            });
        });
    });
}

#[test]
fn feature_intro_becomes_visible_when_target_window_is_assigned() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |_, ctx| {
            let window_id = ctx.window_id();

            OneTimeModalModel::handle(ctx).update(ctx, |model, ctx| {
                // Intro selected before any window is active (no active window
                // available to bind yet).
                model.target_window_id = None;
                model.active_feature_intro = Some(FeatureIntroId::CustomModelRouter);
                assert_eq!(model.active_feature_intro(), None);

                model.update_target_window_id(window_id, ctx);

                assert_eq!(model.target_window_id, Some(window_id));
                assert_eq!(
                    model.active_feature_intro(),
                    Some(FeatureIntroId::CustomModelRouter)
                );
            });
        });
    });
}

#[test]
fn feature_intro_skipped_when_all_seen() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |_, ctx| {
            OneTimeModalModel::handle(ctx).update(ctx, |model, ctx| {
                // Mirror the new-user pre-dismissal: mark every registered intro seen.
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    for intro in FEATURE_INTROS {
                        settings.mark_feature_intro_seen(intro.id.as_key(), ctx);
                    }
                });
                for intro in FEATURE_INTROS {
                    assert!(AISettings::as_ref(ctx).is_feature_intro_seen(intro.id.as_key()));
                }

                assert!(!model.check_and_trigger_feature_intro_modal(ctx));
                assert_eq!(model.active_feature_intro, None);
            });
        });
    });
}
