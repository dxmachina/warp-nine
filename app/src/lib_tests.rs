use super::*;

// LOCAL FORK: `LaunchMode::Tui`/`TuiEntryPoint` went out with the agent CLI, so
// the TUI halves of these tests (api-key plumbing, the `.tui` secure-storage
// suffix, the `LogFrontend::Tui` mapping) have nothing left to protect. The App
// / Test / remote-server halves still do and are kept.

#[test]
fn app_accepts_api_key() {
    let app = LaunchMode::App {
        args: Default::default(),
        api_key: Some("app-api-key".to_owned()),
    };

    assert_eq!(
        api_key_from_launch_mode(&app).as_deref(),
        Some("app-api-key")
    );
}

#[test]
fn app_keeps_default_secure_storage_service_name() {
    let launch_mode = LaunchMode::App {
        args: Default::default(),
        api_key: None,
    };

    assert_eq!(
        launch_mode.secure_storage_service_name("dev.warp.Warp-Dev"),
        "dev.warp.Warp-Dev"
    );
}

#[test]
fn launch_modes_select_expected_logging_frontend() {
    let app = LaunchMode::App {
        args: Default::default(),
        api_key: None,
    };
    let test = LaunchMode::Test {
        driver: Box::new(None),
        is_integration_test: false,
    };

    assert_eq!(app.log_frontend(), LogFrontend::Gui);
    assert_eq!(test.log_frontend(), LogFrontend::Gui);
    assert_eq!(
        LaunchMode::RemoteServerProxy.log_frontend(),
        LogFrontend::Cli
    );
    assert_eq!(
        LaunchMode::RemoteServerDaemon {
            identity_key: "test".to_owned(),
        }
        .log_frontend(),
        LogFrontend::Cli
    );
}
