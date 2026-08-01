//! LOCAL FORK: what is left of tracing setup once cloud-agent OTLP export is gone.
//!
//! Upstream used this module to export spans marked `tags.cloud_agent` to an OTLP
//! collector, minting and refreshing the credentials for it through
//! `warp_managed_secrets`. Export was opt-in behind the
//! `WARP_CLOUD_AGENT_OTLP_ENDPOINT` environment variable: when that variable was
//! absent, `init` installed a no-op subscriber and returned immediately. This fork
//! never sets it, so the early return was the only reachable path, and the
//! exporter, the shutdown-ordered span registry and the credential refresh loop
//! behind it were unreachable in every build.
//!
//! Installing `NoSubscriber` is exactly what upstream did in that same situation,
//! so behaviour is unchanged. Application logging goes through `warp_logging` and
//! is not affected by any of this; the only thing suppressed here is the `tracing`
//! crate emitting span and event lines of its own.
//!
//! `init` used to return an `Initialization` guard that owned the tracer provider
//! and flushed still-open spans at shutdown. With no provider there is nothing to
//! flush, so it returns the unit type and the lifecycle no longer threads a guard
//! through `app_callbacks`.

use tracing::subscriber;

pub fn init() -> anyhow::Result<()> {
    // Configure the global tracing subscriber to not care about any spans or
    // events.
    //
    // This is done so that we prevent the `tracing` crate from writing out log
    // lines for spans and trace events.
    subscriber::set_global_default(subscriber::NoSubscriber::new())?;
    Ok(())
}
