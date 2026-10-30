//! Explicit local stack ownership, separate from ordinary TCP interfaces.

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};
use ratspeak_runtime::network_ownership::{self, NetworkRequest};
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn api_network_ownership(state: State<'_, Arc<AppState>>) -> AppResult<Value> {
    Ok(network_ownership::snapshot(&state))
}

#[tauri::command]
pub async fn test_shared_instance(
    state: State<'_, Arc<AppState>>,
    args: NetworkRequest,
) -> AppResult<Value> {
    network_ownership::test_connection(Arc::clone(&state), args)
        .await
        .map_err(AppError::bad_request)
}

#[tauri::command]
pub async fn set_network_ownership(
    state: State<'_, Arc<AppState>>,
    args: NetworkRequest,
) -> AppResult<Value> {
    let state = Arc::clone(&state);
    tokio::spawn(async move {
        let result = network_ownership::apply(state.clone(), args).await;
        if result.is_ok() && network_ownership::local_interfaces_allowed(&state) {
            crate::commands::ble::restore_ble_peer_if_requested(state).await;
        }
        result
    })
    .await
    .map_err(|_| {
        AppError::internal("Network switch task failed; check network status before retrying.")
    })?
    .map_err(AppError::bad_request)
}

/// The shared instance's access file. Its control key is an admin credential,
/// released with the active identity's passcode or a confirmation.
#[tauri::command]
pub async fn export_shared_access(
    state: State<'_, Arc<AppState>>,
    passcode: Option<String>,
    confirmed: Option<bool>,
) -> AppResult<String> {
    network_ownership::require_developer_mode(&state).map_err(AppError::bad_request)?;
    let active = crate::helpers::active_identity_id(&state);
    if let Some(passcode) = super::identity::release_passcode(&state, &active, passcode, confirmed)?
    {
        super::identity::verify_identity_passcode(&state, &active, passcode).await?;
    }
    network_ownership::export_access(&state).map_err(AppError::bad_request)
}

#[tauri::command]
pub async fn import_shared_access(
    state: State<'_, Arc<AppState>>,
    content: String,
) -> AppResult<Value> {
    network_ownership::require_developer_mode(&state).map_err(AppError::bad_request)?;
    network_ownership::import_access(content).map_err(AppError::bad_request)
}
