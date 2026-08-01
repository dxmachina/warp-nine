//! LOCAL FORK: these tests were rewritten when the `UpdateManager` stopped talking to a
//! server.
//!
//! The file used to hold fifty tests in about 5,100 lines, and forty-nine of them drove a
//! mocked `ObjectClient`: sync-queue state transitions, the conflict machinery that ran
//! when the backend rejected an update as stale, real-time pushes from other clients,
//! guest ACLs, and the notebook edit baton over the wire. None of that code exists any
//! more, so none of those tests could be adapted; there was nothing left for them to
//! assert against.
//!
//! What replaces them covers the behaviour that does exist: every write lands in the
//! in-memory model and in sqlite, synchronously, and stays there. Four of them guard bugs
//! found while making that change and say so.

use warpui::{App, ModelHandle, SingletonEntity};

use super::UpdateManager;
use crate::ASSETS;
use crate::auth::user::TEST_USER_UID;
use crate::cloud_object::CloudObjectTypeAndId;
use crate::cloud_object::model::actions::{ObjectActionType, ObjectActions};
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{CloudObjectEventEntrypoint, Owner};
use crate::persistence::ModelEvent;
use crate::server::cloud_objects::test_utils::{
    UpdateManagerStruct, create_update_manager_struct, initialize_app,
};
use crate::server::cloud_objects::update_manager::get_duplicate_object_name;
use crate::server::ids::{ClientId, ObjectUid, SyncId};
use crate::workflows::workflow::Workflow;

// -------------------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------------------

fn create_notebook(app: &mut App, update_manager: &ModelHandle<UpdateManager>, id: ClientId) {
    update_manager.update(app, |update_manager, ctx| {
        update_manager.create_notebook(
            id,
            Owner::mock_current_user(),
            None,
            Default::default(),
            CloudObjectEventEntrypoint::Unknown,
            true,
            ctx,
        );
    });
}

fn create_workflow(app: &mut App, update_manager: &ModelHandle<UpdateManager>, id: ClientId) {
    update_manager.update(app, |update_manager, ctx| {
        update_manager.create_workflow(
            Workflow::new("client_workflow".to_string(), "echo client".to_string()),
            Owner::mock_current_user(),
            None,
            id,
            CloudObjectEventEntrypoint::Unknown,
            true,
            ctx,
        );
    });
}

fn update_workflow(app: &mut App, update_manager: &ModelHandle<UpdateManager>, sync_id: SyncId) {
    update_manager.update(app, |update_manager, ctx| {
        update_manager.update_workflow(
            Workflow::new("client workflow 2", "echo client 2"),
            sync_id,
            None,
            ctx,
        )
    });
}

#[track_caller]
fn assert_trashed(app: &mut App, uid: &ObjectUid, is_trashed: bool) {
    CloudModel::handle(app).read(app, |cloud_model, _| {
        let object = cloud_model
            .get_by_uid(uid)
            .unwrap_or_else(|| panic!("object {uid} should be in the cloud model"));
        assert_eq!(
            object.metadata().trashed_ts.is_some(),
            is_trashed,
            "Expected trashed status for {uid} to be {is_trashed}"
        );
    });
}

#[track_caller]
fn assert_pending_content_changes(app: &mut App, uid: &ObjectUid, pending: bool) {
    CloudModel::handle(app).read(app, |cloud_model, _| {
        let object = cloud_model
            .get_by_uid(uid)
            .unwrap_or_else(|| panic!("object {uid} should be in the cloud model"));
        assert_eq!(
            object.metadata().has_pending_content_changes(),
            pending,
            "Expected has_pending_content_changes for {uid} to be {pending}"
        );
    });
}

#[track_caller]
fn assert_exists(app: &mut App, uid: &ObjectUid, exists: bool) {
    CloudModel::handle(app).read(app, |cloud_model, _| {
        assert_eq!(
            cloud_model.get_by_uid(uid).is_some(),
            exists,
            "Expected object {uid} to exist: {exists}"
        );
    });
}

fn db_events(update_manager_struct: &UpdateManagerStruct) -> Vec<ModelEvent> {
    let mut events = Vec::new();
    while let Ok(event) = update_manager_struct.receiver.try_recv() {
        events.push(event);
    }
    events
}

fn wrote_an_object(events: &[ModelEvent]) -> bool {
    events
        .iter()
        .any(|event| !matches!(event, ModelEvent::DeleteObjects { .. }))
}

// -------------------------------------------------------------------------------------
// Creation and update
// -------------------------------------------------------------------------------------

#[test]
fn test_create_writes_to_model_and_sqlite() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();

        create_workflow(&mut app, &s.update_manager, client_id);

        assert_exists(&mut app, &client_id.to_string(), true);
        assert!(
            wrote_an_object(&db_events(&s)),
            "creating a workflow should have written it to sqlite"
        );
    });
}

#[test]
fn test_create_sets_editor() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();

        create_notebook(&mut app, &s.update_manager, client_id);

        app.read(|ctx| {
            let object = CloudModel::as_ref(ctx)
                .get_by_uid(&client_id.to_string())
                .expect("notebook should be in the cloud model");
            assert_eq!(
                object.metadata().current_editor_uid.as_deref(),
                Some(TEST_USER_UID)
            );
        });
    });
}

/// LOCAL FORK regression test.
///
/// `update_object` used to call `increment_in_flight_request_count`, and the response
/// handler decremented it. With the response gone the counter could only rise, so every
/// edited object would stay `InFlight` -- which `has_pending_content_changes` reports as
/// unsaved and `num_unsaved_objects_to_warn_about_before_quitting` counts. Every quit
/// after any edit would have warned about unsaved work already written to disk.
#[test]
fn test_update_does_not_leave_the_object_pending() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();

        create_workflow(&mut app, &s.update_manager, client_id);
        assert_pending_content_changes(&mut app, &client_id.to_string(), false);

        update_workflow(&mut app, &s.update_manager, SyncId::ClientId(client_id));
        assert_pending_content_changes(&mut app, &client_id.to_string(), false);

        let unsaved = CloudModel::handle(&app).read(&app, |model, _| model.num_unsaved_objects());
        assert_eq!(unsaved, 0, "an edited object should not count as unsaved");
    });
}

#[test]
fn test_update_writes_the_new_model_to_sqlite() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();

        create_workflow(&mut app, &s.update_manager, client_id);
        let _ = db_events(&s);

        update_workflow(&mut app, &s.update_manager, SyncId::ClientId(client_id));

        assert!(
            wrote_an_object(&db_events(&s)),
            "updating a workflow should have written it to sqlite"
        );
        CloudModel::handle(&app).read(&app, |cloud_model, _| {
            let workflow = cloud_model
                .get_workflow(&SyncId::ClientId(client_id))
                .expect("workflow should exist");
            assert_eq!(workflow.model().data.name(), "client workflow 2");
        });
    });
}

// -------------------------------------------------------------------------------------
// Trash, untrash and delete
// -------------------------------------------------------------------------------------

/// LOCAL FORK regression test.
///
/// `trash_object` opened with `let Some(server_id) = id.server_id() else { return }`. An
/// object created in this build has only a client id, so the method returned before
/// touching anything and the Trash menu entry silently did nothing.
#[test]
fn test_trash_object_created_locally() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();

        create_workflow(&mut app, &s.update_manager, client_id);
        assert_trashed(&mut app, &client_id.to_string(), false);
        let _ = db_events(&s);

        let id = CloudObjectTypeAndId::Workflow(SyncId::ClientId(client_id));
        s.update_manager.update(&mut app, |update_manager, ctx| {
            update_manager.trash_object(id, ctx);
        });

        assert_trashed(&mut app, &client_id.to_string(), true);
        assert!(
            wrote_an_object(&db_events(&s)),
            "trashing should have persisted the trashed timestamp"
        );
    });
}

#[test]
fn test_untrash_object_created_locally() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();

        create_workflow(&mut app, &s.update_manager, client_id);
        let id = CloudObjectTypeAndId::Workflow(SyncId::ClientId(client_id));

        s.update_manager.update(&mut app, |update_manager, ctx| {
            update_manager.trash_object(id, ctx);
        });
        assert_trashed(&mut app, &client_id.to_string(), true);

        s.update_manager.update(&mut app, |update_manager, ctx| {
            update_manager.untrash_object(id, ctx);
        });
        assert_trashed(&mut app, &client_id.to_string(), false);
    });
}

/// LOCAL FORK regression test: `delete_object_by_user` had the same server-id guard.
#[test]
fn test_delete_object_created_locally() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();

        create_workflow(&mut app, &s.update_manager, client_id);
        assert_exists(&mut app, &client_id.to_string(), true);
        let _ = db_events(&s);

        let id = CloudObjectTypeAndId::Workflow(SyncId::ClientId(client_id));
        s.update_manager.update(&mut app, |update_manager, ctx| {
            update_manager.delete_object_by_user(id, ctx);
        });

        assert_exists(&mut app, &client_id.to_string(), false);
        assert!(
            db_events(&s)
                .iter()
                .any(|event| matches!(event, ModelEvent::DeleteObjects { .. })),
            "deleting should have removed the object from sqlite"
        );
    });
}

#[test]
fn test_delete_removes_the_objects_actions() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();

        create_workflow(&mut app, &s.update_manager, client_id);
        let id = CloudObjectTypeAndId::Workflow(SyncId::ClientId(client_id));
        let uid = client_id.to_string();

        s.update_manager.update(&mut app, |update_manager, ctx| {
            update_manager.record_object_action(id, ObjectActionType::Execute, None, ctx);
        });
        let recorded = ObjectActions::handle(&app).update(&mut app, |actions, _| {
            actions.count_actions_for_object(&uid)
        });
        assert_eq!(recorded, 1, "the action should have been recorded");

        s.update_manager.update(&mut app, |update_manager, ctx| {
            update_manager.delete_object_by_user(id, ctx);
        });

        let remaining = ObjectActions::handle(&app).update(&mut app, |actions, _| {
            actions.count_actions_for_object(&uid)
        });
        assert_eq!(remaining, 0, "the action should have gone with the object");
    });
}

// -------------------------------------------------------------------------------------
// The notebook edit baton
// -------------------------------------------------------------------------------------

/// LOCAL FORK regression test.
///
/// Both baton methods opened with `let SyncId::ServerId(server_id) = notebook_id else {
/// return }`, so neither could run on a locally created notebook. Taking the baton is what
/// the "someone else is editing" modal's Take Access button waits on.
#[test]
fn test_grab_and_release_notebook_edit_access_locally() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let s = create_update_manager_struct(&mut app);
        let client_id = ClientId::new();
        let notebook_id = SyncId::ClientId(client_id);

        create_notebook(&mut app, &s.update_manager, client_id);

        s.update_manager.update(&mut app, |update_manager, ctx| {
            update_manager.give_up_notebook_edit_access(notebook_id, ctx);
        });
        app.read(|ctx| {
            let object = CloudModel::as_ref(ctx)
                .get_by_uid(&client_id.to_string())
                .expect("notebook should exist");
            assert_eq!(
                object.metadata().current_editor_uid,
                None,
                "giving up the baton should clear the current editor"
            );
        });

        s.update_manager.update(&mut app, |update_manager, ctx| {
            update_manager.grab_notebook_edit_access(notebook_id, true, ctx);
        });
        app.read(|ctx| {
            let object = CloudModel::as_ref(ctx)
                .get_by_uid(&client_id.to_string())
                .expect("notebook should exist");
            assert_eq!(
                object.metadata().current_editor_uid.as_deref(),
                Some(TEST_USER_UID),
                "grabbing the baton should set the current editor"
            );
        });
    });
}

// -------------------------------------------------------------------------------------
// Naming
// -------------------------------------------------------------------------------------

#[test]
fn test_get_duplicate_object_name() {
    assert_eq!(get_duplicate_object_name("Workflow"), "Workflow (1)");
    assert_eq!(get_duplicate_object_name("Workflow (1)"), "Workflow (2)");
    assert_eq!(get_duplicate_object_name("Workflow (9)"), "Workflow (10)");
    assert_eq!(
        get_duplicate_object_name("Workflow (1) extra"),
        "Workflow (1) extra (1)"
    );
}
