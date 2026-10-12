use super::*;
use crate::icons::GlyphMode;

#[test]
fn glyph_resolution_obeys_precedence() {
    assert_eq!(
        resolve_glyph_mode(
            Some(GlyphMode::Ascii),
            Some("nerd"),
            None,
            None,
            Some("en_US.UTF-8")
        )
        .0,
        GlyphMode::Ascii
    );
    assert_eq!(
        resolve_glyph_mode(None, Some("nerd"), None, None, Some("C")).0,
        GlyphMode::NerdFont
    );
}

#[test]
fn locale_default_distinguishes_utf8() {
    assert_eq!(
        resolve_glyph_mode(None, None, None, None, Some("en_US.UTF-8")).0,
        GlyphMode::NerdFont
    );
    assert_eq!(
        resolve_glyph_mode(None, None, Some("C"), None, Some("en_US.UTF-8")).0,
        GlyphMode::Ascii
    );
}

#[test]
fn invalid_config_warns_and_falls_back() {
    let (mode, warning) = resolve_glyph_mode(None, Some("emoji"), None, None, Some("C.UTF-8"));
    assert_eq!(mode, GlyphMode::NerdFont);
    assert!(warning.unwrap().contains("glyph_mode"));
}

#[test]
fn on_app_exit_defaults_to_when_idle() {
    let config: TuiConfig = toml::from_str("").unwrap();
    assert_eq!(
        config.on_app_exit.stop_managed_daemon,
        StopManagedDaemon::WhenIdle
    );

    let config: TuiConfig = toml::from_str("server_url = \"http://127.0.0.1:1\"").unwrap();
    assert_eq!(
        config.on_app_exit.stop_managed_daemon,
        StopManagedDaemon::WhenIdle
    );
}

#[test]
fn on_app_exit_parses_section_values() {
    let always: TuiConfig = toml::from_str(
        r#"
        [on-app-exit]
        stop_managed_daemon = "always"
        "#,
    )
    .unwrap();
    assert_eq!(
        always.on_app_exit.stop_managed_daemon,
        StopManagedDaemon::Always
    );

    let never: TuiConfig = toml::from_str(
        r#"
        [on-app-exit]
        stop_managed_daemon = "never"
        "#,
    )
    .unwrap();
    assert_eq!(
        never.on_app_exit.stop_managed_daemon,
        StopManagedDaemon::Never
    );
}

#[test]
fn managed_exit_action_follows_policy_and_health() {
    assert_eq!(
        managed_exit_action(StopManagedDaemon::Never, Some(true), true),
        ManagedExitAction::Detach
    );
    assert_eq!(
        managed_exit_action(StopManagedDaemon::WhenIdle, Some(true), true),
        ManagedExitAction::ShutdownIfIdle
    );
    assert_eq!(
        managed_exit_action(StopManagedDaemon::Always, Some(true), false),
        ManagedExitAction::ForceShutdown
    );
    assert_eq!(
        managed_exit_action(StopManagedDaemon::Always, Some(false), true),
        ManagedExitAction::Detach
    );
    assert_eq!(
        managed_exit_action(StopManagedDaemon::Always, None, true),
        ManagedExitAction::TerminateOwned
    );
    assert_eq!(
        managed_exit_action(StopManagedDaemon::WhenIdle, None, false),
        ManagedExitAction::Detach
    );
}

#[test]
fn mouse_defaults_to_enabled_and_can_be_disabled_in_existing_configs() {
    assert!(TuiConfig::default().mouse_enabled);
    assert!(
        toml::from_str::<TuiConfig>("server_url = \"http://127.0.0.1:1\"")
            .unwrap()
            .mouse_enabled
    );
    let disabled: TuiConfig = toml::from_str("mouse_enabled = false").unwrap();
    assert!(!disabled.mouse_enabled);
    let round_trip: TuiConfig = toml::from_str(&toml::to_string(&disabled).unwrap()).unwrap();
    assert!(!round_trip.mouse_enabled);
}
