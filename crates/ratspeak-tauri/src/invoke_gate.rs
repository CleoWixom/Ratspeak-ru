//! One admission check in front of every IPC command, from in-memory state
//! only. Commands keep their own checks as defense in depth.

use std::sync::Arc;

use tauri::Manager;
use tauri::ipc::Invoke;

use crate::error::AppError;
use crate::helpers;
use crate::state::AppState;

/// Commands that still answer while no identity is active: the shell,
/// platform probes, startup and setup, app settings and identity management.
/// The gate refuses every other command once it knows none is active.
pub(crate) const IDENTITY_FREE: &[&str] = &[
    // Shell.
    "open_external_url",
    "open_support_email",
    "set_window_decorations",
    "save_image_to_photos",
    "request_microphone_permission",
    "open_mobile_app_settings",
    // Platform and runtime probes, read once at page load.
    "api_ble_available",
    "api_ble_peer_available",
    "api_lxmf_limits",
    "voice_status",
    // Startup, setup and reset.
    "api_version",
    "api_startup_progress",
    "api_setup_status",
    "api_setup_complete",
    "api_setup_restart",
    "api_set_foreground",
    "api_factory_reset",
    // App settings.
    "api_app_settings",
    "api_notification_settings",
    "set_appearance",
    "set_native_theme",
    "set_text_scale",
    "set_desktop_notifications",
    "set_hide_known_spam_peers",
    "set_lxmf_limit_1mb",
    "set_peers_sort",
    "accept_public_channel_consent",
    "set_auto_announce",
    "set_hardware_lock_timeout",
    "set_developer_mode",
    "set_announce_ratspeak_usage",
    "set_android_ble_rnode_auto_resume",
    "set_activity_identity_protection",
    // Identities.
    "api_identity",
    "api_list_identities",
    "api_create_identity",
    "api_import_identity",
    "api_import_identity_base64",
    "api_preview_identity_import_base64",
    "restore_seed_identity",
    "switch_identity",
    "api_activate_identity",
    "api_update_identity",
    "api_delete_identity",
    "unlock_identity",
    "set_identity_passcode",
    "remove_identity_passcode",
    "reveal_identity_mnemonic",
    "api_export_identity",
    "api_export_identity_base64",
    "api_export_identity_backup_base64",
    "api_export_identity_reticulum_base64",
    "api_export_identity_reticulum_base32",
    "hw_detect",
    "hw_provision_recoverable",
    "hw_provision_hardware_only",
    "hw_import_existing",
    "hw_restore",
    "hw_stage_unlock",
    "hw_reset_piv",
    "hw_change_pin",
    "hw_remove",
    "hw_unlock",
    "hw_activate_and_unlock",
];

/// Wraps the generated command handler in the admission check.
pub fn gated<R: tauri::Runtime>(
    commands: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let refused = invoke
            .message
            .webview_ref()
            .try_state::<Arc<AppState>>()
            .and_then(|state| refusal(&state, invoke.message.command()));
        match refused {
            Some(error) => {
                invoke.resolver.reject(error);
                true
            }
            None => commands(invoke),
        }
    }
}

/// Why `command` must not run now, if it must not.
fn refusal(state: &AppState, command: &str) -> Option<AppError> {
    if !IDENTITY_FREE.contains(&command) && helpers::active_identity_known_absent(state) {
        return Some(AppError::conflict("no active identity"));
    }
    None
}
