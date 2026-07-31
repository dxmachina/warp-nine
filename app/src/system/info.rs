use std::ffi::OsStr;

use byte_unit::Byte;
use sysinfo::ProcessesToUpdate;
use warpui::{App, AppContext, Entity, ModelContext, SingletonEntity};

use crate::system::memory_footprint;

/// The threshold at which we emit a memory usage warning.
const MEMORY_USAGE_WARNING_THRESHOLD: Option<Byte> = byte_unit::Byte::GIGABYTE.multiply(10);

/// The refresh interval for system information, in seconds.
const REFRESH_INTERVAL_S: usize = 5;
/// The refresh interval for system information.
const REFRESH_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(REFRESH_INTERVAL_S as u64);

pub enum SystemInfoEvent {
    /// There is new system info available for consumers to query.
    Refreshed,
    /// The application is using a large quantity of memory.
    MemoryUsageHigh,
}

pub struct SystemInfo {
    /// A structure we can use to efficiently query system information.
    system: sysinfo::System,
    /// Whether or not we've already emitted an event due to high memory usage.
    has_emitted_memory_warning_event: bool,
    /// The long OS version.
    long_os_version: Option<String>,
}

impl SystemInfo {
    /// Creates a new [`SystemInfo`] model and begins periodic fetching of
    /// system information.
    ///
    /// Currently only retrieves and exposes memory usage information for the
    /// current process.
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let mut me = Self {
            system: sysinfo::System::new(),
            has_emitted_memory_warning_event: false,
            long_os_version: sysinfo::System::long_os_version(),
        };

        // Initialize the underlying system info.  This is necessary in order
        // for our first read of CPU stats to be accurate, as they are computed
        // as a delta between the previous refresh and now.
        me.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Self::current_pid()]),
            false, /* refresh_dead_processes */
            Self::refresh_kind(),
        );

        // If we're doing automated heap usage tracking, set up periodic
        // refreshes of the memory usage data.
        Self::schedule_refresh(ctx);

        me
    }

    // LOCAL FORK: this counted blocks towards the periodic resource-usage telemetry
    // report, which went with telemetry. Kept as a no-op so the terminal view call site
    // is unchanged.
    pub fn handle_block_created(&mut self) {}

    /// Returns the amount of memory being used by the current process, in
    /// bytes.
    pub fn used_memory(&self) -> Byte {
        self.system
            .process(Self::current_pid())
            .expect("current process should exist")
            .memory()
            .into()
    }

    /// Returns the full memory footprint of the current process, in bytes.
    ///
    /// Unlike [`used_memory`] (RSS), this includes memory that has been
    /// swapped out or compressed by the OS.  On macOS this matches the value
    /// shown by Activity Monitor.
    pub fn memory_footprint(&self) -> Byte {
        memory_footprint::memory_footprint_bytes().into()
    }

    /// Returns the average CPU usage over the refresh interval.
    ///
    /// If one CPU core is utilized at 100%, this will return 1.  It may return
    /// a value >1 on multi-core machines.
    pub fn cpu_usage(&self) -> f32 {
        let total_usage = self
            .system
            .process(Self::current_pid())
            .expect("current process should exist")
            .cpu_usage();
        total_usage / 100.
    }

    pub fn long_os_version(&self) -> Option<&str> {
        self.long_os_version.as_deref()
    }

    fn schedule_refresh(ctx: &mut ModelContext<Self>) {
        ctx.spawn(
            async {
                warpui::r#async::Timer::after(REFRESH_INTERVAL).await;
            },
            |me, _, ctx| {
                me.refresh(ctx);
                Self::schedule_refresh(ctx);
            },
        );
    }

    fn refresh(&mut self, ctx: &mut ModelContext<Self>) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Self::current_pid()]),
            false, /* refresh_dead_processes */
            Self::refresh_kind(),
        );
        ctx.emit(SystemInfoEvent::Refreshed);

        let footprint = self.memory_footprint();
        self.check_for_excessive_memory_usage(footprint, ctx);
    }

    /// Checks for excessive memory usage.  This emits [`SystemInfoEvent::MemoryUsageHigh`]
    /// and triggers a Sentry heap profile dump if excessive usage is detected.
    ///
    /// The threshold check uses `memory_footprint` (which includes swapped
    /// and compressed pages) so we actually detect high memory situations.
    fn check_for_excessive_memory_usage(
        &mut self,
        memory_footprint: Byte,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.has_emitted_memory_warning_event {
            return;
        }

        // Use footprint (not RSS) for the threshold so we catch memory
        // that has been swapped out or compressed by the OS.
        if memory_footprint
            < MEMORY_USAGE_WARNING_THRESHOLD.expect("Threshold should not overflow u64")
        {
            return;
        }

        // If we're tracking heap usage and detect excessive memory usage,
        // dump and upload the current heap profiling data.
        #[cfg(feature = "heap_usage_tracking")]
        {
            let breakdown_for_sentry = memory_footprint::memory_breakdown();
            ctx.spawn(
                crate::profiling::dump_jemalloc_heap_profile(breakdown_for_sentry),
                |_, _, _| {},
            );
        }

        ctx.emit(SystemInfoEvent::MemoryUsageHigh);
        self.has_emitted_memory_warning_event = true;
    }

    /// Returns the pid of the current process.
    fn current_pid() -> sysinfo::Pid {
        sysinfo::get_current_pid().expect("Platform should support process IDs")
    }

    /// Returns the [`sysinfo::ProcessRefreshKind`] that should be used when
    /// retrieving information about the current process.
    fn refresh_kind() -> sysinfo::ProcessRefreshKind {
        sysinfo::ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn refresh_all_processes(&mut self) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true, /* remove_dead_processes */
            Self::refresh_kind(),
        );
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn processes_by_name<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a sysinfo::Process> {
        self.system.processes_by_name(OsStr::new(name))
    }
}

impl Entity for SystemInfo {
    type Event = SystemInfoEvent;
}

impl SingletonEntity for SystemInfo {}

// LOCAL FORK: ResourceUsageReporter, its CPU/memory stat structs and the sample
// buffer they aggregated all existed to build the periodic resource-usage telemetry
// report. They went with telemetry.
