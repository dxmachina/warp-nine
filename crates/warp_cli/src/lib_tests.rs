use std::ffi::OsString;

use clap::Parser;

use super::*;

#[test]
fn identifies_worker_subcommands() {
    assert!(is_worker_invocation("minidump-server"));
    #[cfg(unix)]
    assert!(is_worker_invocation(&terminal_server_subcommand()));
    #[cfg(feature = "plugin_host")]
    assert!(is_worker_invocation("--plugin-host"));
    assert!(!is_worker_invocation("--prompt"));
}

fn set_env_var(name: &str, value: &str) -> Option<OsString> {
    let previous = std::env::var_os(name);
    // Safety: tests that mutate process environment are marked `serial` so we
    // do not race with other environment readers/writers in this crate.
    unsafe { std::env::set_var(name, value) };
    previous
}

fn restore_env_var(name: &str, previous: Option<OsString>) {
    match previous {
        // Safety: tests that mutate process environment are marked `serial` so
        // we do not race with other environment readers/writers in this crate.
        Some(value) => unsafe { std::env::set_var(name, value) },
        // Safety: tests that mutate process environment are marked `serial` so
        // we do not race with other environment readers/writers in this crate.
        None => unsafe { std::env::remove_var(name) },
    }
}

#[test]
fn api_key_before_subcommand_parses() {
    // Regression test: `warp --api-key KEY <subcommand>` should work.
    // Previously the top-level [URLS] positional would swallow the subcommand
    // when --api-key preceded it.
    let args = Args::try_parse_from(["warp", "--api-key", "test-key", "dump-debug-info"]).unwrap();

    assert_eq!(args.api_key(), Some(&"test-key".to_string()));
    assert!(matches!(args.command, Some(Command::DumpDebugInfo)));
}

#[test]
fn debug_before_subcommand_parses() {
    // Regression test: `warp --debug <subcommand>` should work.
    // Global flags like --debug must not prevent subcommand detection.
    let args = Args::try_parse_from(["warp", "--debug", "dump-debug-info"]).unwrap();

    assert!(args.debug());
    assert!(matches!(args.command, Some(Command::DumpDebugInfo)));
}

#[test]
fn multiple_global_flags_before_subcommand_parse() {
    // Both --api-key and --debug before the subcommand should work.
    let args = Args::try_parse_from([
        "warp",
        "--api-key",
        "test-key",
        "--debug",
        "dump-debug-info",
    ])
    .unwrap();

    assert_eq!(args.api_key(), Some(&"test-key".to_string()));
    assert!(args.debug());
    assert!(matches!(args.command, Some(Command::DumpDebugInfo)));
}

#[test]
fn completions_parses() {
    let args = Args::try_parse_from(["warp", "completions", "zsh"]).unwrap();

    let Some(Command::Completions { shell }) = args.command else {
        panic!("Expected `completions` command");
    };
    assert_eq!(shell, Some(clap_complete::aot::Shell::Zsh));
}

/// LOCAL FORK: the agent and cloud subcommands are not part of this build.
/// Guard against them coming back.
#[test]
fn agent_and_cloud_subcommands_are_absent() {
    let command = Args::clap_command();
    for name in [
        "agent",
        "api-key",
        "artifact",
        "environment",
        "federate",
        "harness-support",
        "integration",
        "login",
        "logout",
        "mcp",
        "memory",
        "memory-store",
        "model",
        "provider",
        "run",
        "runner",
        "schedule",
        "secret",
        "whoami",
    ] {
        assert!(
            command.find_subcommand(name).is_none(),
            "`{name}` should not be a subcommand"
        );
    }
}

#[test]
#[serial_test::serial]
fn hidden_server_overrides_parse_from_env() {
    let previous_server_root = set_env_var(SERVER_ROOT_URL_OVERRIDE_ENV, "http://localhost:8080");
    let previous_ws = set_env_var(WS_SERVER_URL_OVERRIDE_ENV, "ws://localhost:8082/graphql/v2");
    let previous_session_sharing = set_env_var(
        SESSION_SHARING_SERVER_URL_OVERRIDE_ENV,
        "ws://127.0.0.1:8081",
    );

    let args = Args::try_parse_from(["warp", "dump-debug-info"]).unwrap();

    restore_env_var(SERVER_ROOT_URL_OVERRIDE_ENV, previous_server_root);
    restore_env_var(WS_SERVER_URL_OVERRIDE_ENV, previous_ws);
    restore_env_var(
        SESSION_SHARING_SERVER_URL_OVERRIDE_ENV,
        previous_session_sharing,
    );

    assert_eq!(args.server_root_url(), Some("http://localhost:8080"));
    assert_eq!(args.ws_server_url(), Some("ws://localhost:8082/graphql/v2"));
    assert_eq!(
        args.session_sharing_server_url(),
        Some("ws://127.0.0.1:8081")
    );
}
