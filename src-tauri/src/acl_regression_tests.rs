//! Verify the ACL compiled into the application, without starting its setup,
//! opening a window, or accessing any user database or credential storage.
use tauri::ipc::Origin;

fn allowed(command: &str, window: &str, origin: Origin) -> bool {
    let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    context
        .runtime_authority_mut()
        .resolve_access(command, window, window, &origin)
        .is_some()
}

#[test]
fn main_window_can_listen_for_tray_events() {
    assert!(allowed("plugin:event|listen", "main", Origin::Local));
}

#[test]
fn main_window_can_remove_tray_listeners() {
    assert!(allowed("plugin:event|unlisten", "main", Origin::Local));
}

#[test]
fn other_windows_do_not_inherit_event_permissions() {
    for command in ["plugin:event|listen", "plugin:event|unlisten"] {
        assert!(!allowed(command, "untrusted", Origin::Local));
    }
}

#[test]
fn remote_origins_cannot_listen_to_application_events() {
    for command in ["plugin:event|listen", "plugin:event|unlisten"] {
        assert!(!allowed(
            command,
            "main",
            Origin::Remote {
                url: "https://example.invalid/".parse().unwrap(),
            },
        ));
    }
}

#[test]
fn event_subscription_does_not_grant_emit_or_shell_permissions() {
    for command in [
        "plugin:event|emit",
        "plugin:event|emit_to",
        "plugin:shell|execute",
        "plugin:shell|spawn",
        "plugin:shell|open",
    ] {
        assert!(!allowed(command, "main", Origin::Local));
    }
}

#[test]
fn destructive_ui_confirmation_does_not_require_native_dialog_permissions() {
    for command in ["plugin:dialog|confirm", "plugin:dialog|ask"] {
        assert!(!allowed(command, "main", Origin::Local));
    }
}
