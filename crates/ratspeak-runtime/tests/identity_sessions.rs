//! Which identity startup opens, through the real startup path. Startup stops
//! at Reticulum, which has no configuration here, so no runtime starts; these
//! tests still share no process-wide state with the library's.

use std::sync::Arc;

use ratspeak_runtime::config::DashboardConfig;
use ratspeak_runtime::db;
use ratspeak_runtime::lxmf::{self, LxmfManager};
use ratspeak_runtime::state::AppState;

fn identity_state() -> (tempfile::TempDir, Arc<AppState>) {
    let root = tempfile::tempdir().unwrap();
    let data_root = root.path().to_path_buf();
    lxmf::initialize_storage_namespace(&data_root).unwrap();
    let config = DashboardConfig {
        data_root: data_root.clone(),
        data_dir: data_root.join(".ratspeak"),
        rns_config_dir: data_root.join("rns"),
        rns_config_dir_overridden: true,
        max_log_entries: 20,
    };
    let pool = db::init_pool(&data_root).unwrap();
    db::init_schema(&pool).unwrap();
    let state = AppState::new(
        config,
        pool,
        Arc::new(ratspeak_core::NoopEmitter),
        Arc::new(ratspeak_core::NoopNotifier),
    );
    (root, Arc::new(state))
}

fn import(state: &AppState, nickname: &str) -> String {
    let key = rns_identity::identity::Identity::new()
        .get_private_key()
        .unwrap()
        .to_vec();
    LxmfManager::import_identity_to_data_dir(&state.config.data_dir, &key, nickname, &state.db)
        .unwrap()
        .0
}

fn profile(state: &AppState, identity: &str) -> std::path::PathBuf {
    state.config.data_dir.join("identities").join(identity)
}

fn active(state: &AppState) -> Option<String> {
    db::get_active_identity(&state.db)
        .unwrap()
        .and_then(|row| row.get("hash").and_then(|v| v.as_str()).map(str::to_owned))
}

fn loaded(state: &AppState) -> Option<String> {
    state
        .lxmf
        .lock()
        .unwrap()
        .as_ref()
        .map(|mgr| mgr.identity_hash.clone())
}

fn set_last_used(state: &AppState, identity: &str, at: f64) {
    state
        .db
        .get()
        .unwrap()
        .execute(
            "UPDATE identities SET last_used = ?1 WHERE hash = ?2",
            rusqlite::params![at, identity],
        )
        .unwrap();
}

async fn start(state: &Arc<AppState>) {
    ratspeak_runtime::init_rns_lxmf(Arc::clone(state), state.config.data_root.clone()).await;
}

#[tokio::test]
async fn startup_recovery_starts_a_new_identity_session() {
    let (_root, state) = identity_state();
    let hash = import(&state, "Found");
    state
        .db
        .get()
        .unwrap()
        .execute("DELETE FROM identities", [])
        .unwrap();
    let generation = state.current_identity_session_generation();
    start(&state).await;
    assert_eq!(active(&state).as_deref(), Some(hash.as_str()));
    assert!(state.current_identity_session_generation() > generation);
}

// With no active row the most recently used identity opens, as after a
// removal; a protected one opens to its lock sheet.
#[tokio::test]
async fn a_cold_start_without_an_active_row_opens_the_most_recent_identity() {
    let (_root, state) = identity_state();
    let older = import(&state, "Older");
    let newer = import(&state, "Newer");
    let newer_profile = profile(&state, &newer);
    std::fs::rename(
        newer_profile.join("identity"),
        newer_profile.join("identity.enc"),
    )
    .unwrap();
    set_last_used(&state, &older, 1.0);
    set_last_used(&state, &newer, 2.0);
    let generation = state.current_identity_session_generation();
    start(&state).await;
    assert_eq!(active(&state).as_deref(), Some(newer.as_str()));
    assert_eq!(state.hw_locked_hash().as_deref(), Some(newer.as_str()));
    assert!(state.current_identity_session_generation() > generation);
}

// A plain key that no longer loads is passed over: activating it would keep
// every later start on it, and the usable identities would never be tried.
#[tokio::test]
async fn a_cold_start_passes_over_an_identity_whose_key_does_not_load() {
    let (_root, state) = identity_state();
    let older = import(&state, "Older");
    let newer = import(&state, "Newer");
    std::fs::write(profile(&state, &newer).join("identity"), b"short").unwrap();
    set_last_used(&state, &older, 1.0);
    set_last_used(&state, &newer, 2.0);
    start(&state).await;
    assert_eq!(active(&state).as_deref(), Some(older.as_str()));
    assert_eq!(loaded(&state).as_deref(), Some(older.as_str()));
}

// Only an activation marks an identity used: one created or imported but
// never opened doesn't take over from the one in daily use.
#[test]
fn only_an_activation_marks_an_identity_used() {
    let (_root, state) = identity_state();
    let used = import(&state, "Used");
    db::set_active_identity(&state.db, &used).unwrap();
    let imported = import(&state, "Imported");
    let row = db::get_identity(&state.db, &imported).unwrap();
    assert_eq!(row["last_used"], 0.0, "an import is not a use");
    let lxmf_hash = db::get_identity(&state.db, &used).unwrap()["lxmf_hash"]
        .as_str()
        .unwrap()
        .to_owned();
    db::save_identity(&state.db, &used, &lxmf_hash, "Used", "Renamed");
    assert_eq!(
        ratspeak_runtime::most_recent_identity(&state.db, &state.config.data_dir).unwrap(),
        Some(used),
    );
}
