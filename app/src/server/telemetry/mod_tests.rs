use rudder_message::Track;
use virtual_fs::VirtualFS;

use super::*;

// Tests that events with UGC are not persisted to disk.
//
// LOCAL FORK: this used to drive the UGC filter through
// `flush_and_persist_events_at_path`. Telemetry is hard-off in this build:
// `PrivacySettingsSnapshot::should_disable_telemetry()` is a constant `true`
// (`settings/privacy.rs`) that deliberately ignores the stored settings, so it is `true`
// even for `PrivacySettingsSnapshot::mock()`, which sets every field to enabled. That
// function now returns before it reaches `File::create`, and the original assertions
// failed on `File::open` with ENOENT rather than on anything to do with UGC.
//
// So this now pins both halves of the fork's real contract: nothing is written to disk no
// matter what the privacy snapshot claims, and the UGC filter in `persist_events_at_path`
// (which does not consult the snapshot, and is unchanged) still drops UGC events. If
// telemetry is ever re-enabled, restore the original from `git show main:` on this file.
#[test]
fn test_persist_events_doesnt_include_ugc_events() {
    let telemetry_api = TelemetryApi::new();

    VirtualFS::test(
        "test_persist_events_doesnt_include_ugc_events",
        |dirs, _sandbox| {
            // `warpui::telemetry` queues into a process-global event store, so start from
            // a known-empty queue.
            let _ = warpui::telemetry::flush_events();

            // Add one event without UGC
            let user_id = Some("user".into());
            let anonymous_id = "anonymous_id".to_owned();

            warpui::telemetry::record_event(
                user_id.clone(),
                anonymous_id.clone(),
                "non UGC event name".into(),
                None,  /* payload */
                false, /* contains_ugc  */
                warpui::time::get_current_time(),
            );

            warpui::telemetry::record_event(
                user_id.clone(),
                anonymous_id.clone(),
                "UGC event name".into(),
                None, /* payload */
                true, /* contains_ugc  */
                warpui::time::get_current_time(),
            );

            let file_path = dirs.root().join("rudderstack");

            // Telemetry is disabled, so this is a no-op: it reports success, writes no
            // file, and leaves the queued events alone.
            telemetry_api
                .flush_and_persist_events_at_path(10, PrivacySettingsSnapshot::mock(), &file_path)
                .expect("Persisting should succeed while telemetry is disabled");
            assert!(
                !file_path.exists(),
                "no telemetry file should be written while telemetry is disabled"
            );

            // Drive the UGC filter directly. The events are still queued; if the call above
            // had drained them, the file below would come back empty.
            let file = File::create(&file_path).expect("Failed to create file");
            telemetry_api
                .persist_events_at_path(&file, 10, warpui::telemetry::flush_events())
                .expect("Should be able to persist events");
            drop(file);

            let file_content: Vec<RudderBatchMessage> =
                serde_json::from_reader(File::open(file_path).expect("Failed to open file"))
                    .expect("Failed to parse file");

            assert_eq!(file_content.len(), 1);

            let track = file_content[0].unwrap_track();
            assert_eq!(track.event, "non UGC event name");
        },
    );
}

impl RudderBatchMessage {
    fn unwrap_track(&self) -> &Track {
        match self {
            RudderBatchMessage::Track(track) => track,
            _ => panic!("Expected a track event"),
        }
    }
}
