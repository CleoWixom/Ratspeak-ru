//! Auto-lock with a real runtime. The LXMF tick fires the lock, and another
//! runtime's shutdown can hold that tick through process-wide download locks,
//! so this test has a binary of its own.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ratspeak_core::EmitError;
use ratspeak_tauri::state::{AppState, ContinuousInstant};
use ratspeak_tauri::{config::DashboardConfig, db};
use serde_json::Value;
use tauri::Manager;
use tauri::test::{mock_builder, mock_context, noop_assets};

const PASSCODE: &str = "correct horse";

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

impl ratspeak_core::Emitter for Recorder {
    fn try_emit(&self, event: &str, payload: Value) -> Result<(), EmitError> {
        self.0.lock().unwrap().push((event.to_owned(), payload));
        Ok(())
    }
}

// An auto-lock that fell due while the system slept fires from the LXMF tick:
// the passcode identity locks, and its unlock goes with it.
#[test]
fn a_passcode_identity_locks_once_its_auto_lock_fell_due() {
    let root = tempfile::tempdir().unwrap();
    let data_root = root.path().to_path_buf();
    ratspeak_runtime::lxmf::initialize_storage_namespace(&data_root).unwrap();
    let rns_dir = data_root.join("rns");
    std::fs::create_dir_all(&rns_dir).unwrap();
    std::fs::write(
        rns_dir.join("config"),
        "[reticulum]\nshare_instance = No\nenable_transport = No\n\n[interfaces]\n",
    )
    .unwrap();
    let config = DashboardConfig {
        data_root: data_root.clone(),
        data_dir: data_root.join(".ratspeak"),
        rns_config_dir: rns_dir,
        rns_config_dir_overridden: true,
        max_log_entries: 20,
    };
    let pool = db::init_pool(&data_root).unwrap();
    db::init_schema(&pool).unwrap();
    let recorder = Arc::new(Recorder::default());
    let state = Arc::new(AppState::new(
        config,
        pool,
        Arc::clone(&recorder) as Arc<dyn ratspeak_core::Emitter>,
        Arc::new(ratspeak_core::NoopNotifier),
    ));
    let app = mock_builder()
        .manage(Arc::clone(&state))
        .build(mock_context(noop_assets()))
        .unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let loaded = || {
        state
            .lxmf
            .lock()
            .unwrap()
            .as_ref()
            .map(|mgr| mgr.identity_hash.clone())
    };
    let launch = || {
        runtime.block_on(ratspeak_runtime::init_rns_lxmf(
            Arc::clone(&state),
            state.config.data_root.clone(),
        ))
    };

    let key = rns_identity::identity::Identity::new()
        .get_private_key()
        .unwrap()
        .to_vec();
    let (a, _) = ratspeak_runtime::lxmf::LxmfManager::import_identity_to_data_dir(
        &state.config.data_dir,
        &key,
        "A",
        &state.db,
    )
    .unwrap();
    runtime
        .block_on(ratspeak_tauri::commands::identity::set_identity_passcode(
            app.state(),
            ratspeak_tauri::commands::identity::SetPasscodeArgs {
                hash: a.clone(),
                passcode: PASSCODE.to_owned(),
                current: None,
            },
        ))
        .unwrap();
    db::try_set_setting(&state.db, "hardware_session_timeout", "60").unwrap();
    db::set_active_identity(&state.db, &a).unwrap();
    launch();
    let unlocked = runtime
        .block_on(ratspeak_tauri::commands::identity::unlock_identity(
            app.state(),
            PASSCODE.into(),
        ))
        .unwrap();
    assert_eq!(unlocked["ok"], true, "{unlocked}");
    assert_eq!(loaded().as_deref(), Some(a.as_str()));
    {
        let mut armed = state.auto_lock.lock().unwrap();
        let lock = armed.as_mut().expect("the unlock armed no auto-lock");
        assert_eq!(
            (lock.kind, lock.timeout),
            ("passcode", Duration::from_secs(60))
        );
        // Unlocked an hour ago, before a long sleep.
        lock.unlocked_at = ContinuousInstant::now()
            .checked_sub(Duration::from_secs(3600))
            .unwrap();
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while state.hw_locked_hash().is_none() {
        assert!(std::time::Instant::now() < deadline, "no auto-lock");
        std::thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(state.hw_locked_hash().as_deref(), Some(a.as_str()));
    assert!(loaded().is_none());
    let events = recorder.0.lock().unwrap().clone();
    let (_, locked) = events
        .iter()
        .rev()
        .find(|(event, _)| event == "hardware_locked")
        .expect("no lock event");
    assert_eq!(locked["kind"], "passcode");
    assert_eq!(locked["reason"], "timeout");
    launch();
    assert_eq!(
        state.hw_locked_hash().as_deref(),
        Some(a.as_str()),
        "the unlock outlived the lock"
    );
    assert!(loaded().is_none());
    let _ = runtime.block_on(ratspeak_runtime::shutdown_rns_lxmf(&state));
}
