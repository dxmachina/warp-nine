pub mod cloud_objects;
pub mod experiments;
pub mod graphql;
// Runner-only: the minter is constructed solely in the native, ambient-agent-run
// code path (see lib.rs), so it doesn't compile or ship on wasm.
#[cfg(not(target_family = "wasm"))]
pub mod ids;
pub mod network_log_pane_manager;
pub mod network_log_view;
pub mod retry_strategies;
pub mod server_api;
// LOCAL FORK: `sync_queue` went with cloud sync. It was the outbound half of the cloud
// object write path: every create, update and object action was turned into a `QueueItem`,
// ordered so a child never went out before its parent had a server id, retried on failure,
// and replayed from sqlite at startup for anything that had not made it out before the last
// quit. Writes complete when they are made now, so there is nothing to order, retry or
// replay. (`warp_core::sync_queue` is a different, unrelated type: a generic streaming task
// queue that code review uses for file invalidation. It stays.)
pub mod telemetry;

pub use warp_core::operating_system_info::OperatingSystemInfo;
