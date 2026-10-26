//! Auto-lock with a real runtime. Another runtime's shutdown can hold this
//! one up through process-wide download locks, and the identity commands
//! share the process-wide vault, so these tests have a binary of their own
//! and run one at a time.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ratspeak_core::EmitError;
use ratspeak_tauri::state::{AppState, ContinuousInstant};
use ratspeak_tauri::{config::DashboardConfig, db};
use serde_json::Value;
use tauri::Manager;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

const PASSCODE: &str = "correct horse";
const OTHER_PASSCODE: &str = "battery staple";
const CONFIG: &str = "[reticulum]\nshare_instance = No\nenable_transport = No\n\n[interfaces]\n";

static SERIAL: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

impl ratspeak_core::Emitter for Recorder {
    fn try_emit(&self, event: &str, payload: Value) -> Result<(), EmitError> {
        self.0.lock().unwrap().push((event.to_owned(), payload));
        Ok(())
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    rns_dir: std::path::PathBuf,
    state: Arc<AppState>,
    recorder: Arc<Recorder>,
    app: tauri::App<MockRuntime>,
    runtime: tokio::runtime::Runtime,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let data_root = root.path().to_path_buf();
        ratspeak_runtime::lxmf::initialize_storage_namespace(&data_root).unwrap();
        let rns_dir = data_root.join("rns");
        std::fs::create_dir_all(&rns_dir).unwrap();
        std::fs::write(rns_dir.join("config"), CONFIG).unwrap();
        let config = DashboardConfig {
            data_root: data_root.clone(),
            data_dir: data_root.join(".ratspeak"),
            rns_config_dir: rns_dir.clone(),
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
        Self {
            _root: root,
            rns_dir,
            state,
            recorder,
            app,
            runtime,
        }
    }

    /// A plain identity, as an import leaves it.
    fn import(&self, name: &str) -> String {
        let key = rns_identity::identity::Identity::new()
            .get_private_key()
            .unwrap()
            .to_vec();
        ratspeak_runtime::lxmf::LxmfManager::import_identity_to_data_dir(
            &self.state.config.data_dir,
            &key,
            name,
            &self.state.db,
        )
        .unwrap()
        .0
    }

    fn set_passcode(&self, hash: &str, passcode: &str, current: Option<&str>) {
        self.runtime
            .block_on(ratspeak_tauri::commands::identity::set_identity_passcode(
                self.app.state(),
                ratspeak_tauri::commands::identity::SetPasscodeArgs {
                    hash: hash.to_owned(),
                    passcode: passcode.to_owned(),
                    current: current.map(str::to_owned),
                },
            ))
            .unwrap();
    }

    fn remove_passcode(&self, hash: &str, passcode: &str) {
        self.runtime
            .block_on(
                ratspeak_tauri::commands::identity::remove_identity_passcode(
                    self.app.state(),
                    ratspeak_tauri::commands::identity::RemovePasscodeArgs {
                        hash: hash.to_owned(),
                        passcode: passcode.to_owned(),
                    },
                ),
            )
            .unwrap();
    }

    fn launch(&self) {
        self.runtime.block_on(ratspeak_runtime::init_rns_lxmf(
            Arc::clone(&self.state),
            self.state.config.data_root.clone(),
        ));
    }

    fn unlock(&self, passcode: &str) {
        let unlocked = self
            .runtime
            .block_on(ratspeak_tauri::commands::identity::unlock_identity(
                self.app.state(),
                passcode.into(),
            ))
            .unwrap();
        assert_eq!(unlocked["ok"], true, "{unlocked}");
    }

    fn switch(&self, hash: &str) {
        self.runtime
            .block_on(ratspeak_tauri::commands::identity::api_activate_identity(
                self.app.state(),
                hash.to_owned(),
            ))
            .unwrap();
    }

    /// A passcode identity with the auto-lock set to `timeout`, unlocked.
    fn unlocked_passcode_identity(&self, timeout: &str) -> String {
        let hash = self.import("A");
        self.set_passcode(&hash, PASSCODE, None);
        db::try_set_setting(&self.state.db, "hardware_session_timeout", timeout).unwrap();
        db::set_active_identity(&self.state.db, &hash).unwrap();
        self.launch();
        assert_eq!(self.state.hw_locked_hash().as_deref(), Some(hash.as_str()));
        self.unlock(PASSCODE);
        assert_eq!(self.loaded().as_deref(), Some(hash.as_str()));
        hash
    }

    fn loaded(&self) -> Option<String> {
        self.state
            .lxmf
            .lock()
            .unwrap()
            .as_ref()
            .map(|mgr| mgr.identity_hash.clone())
    }

    fn armed(&self) -> Option<ratspeak_tauri::state::AutoLock> {
        self.state.auto_lock.lock().unwrap().clone()
    }

    /// Moves the armed lock's unlock `by` into the past, as a sleep would.
    fn backdate(&self, by: Duration) {
        let mut armed = self.state.auto_lock.lock().unwrap();
        let lock = armed.as_mut().expect("no auto-lock is armed");
        lock.unlocked_at = ContinuousInstant::now().checked_sub(by).unwrap();
    }

    /// Waits for an auto-lock to finish: its event comes after the teardown.
    fn wait_until_locked(&self, within: Duration) {
        let deadline = std::time::Instant::now() + within;
        while !self
            .recorder
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(event, payload)| event == "hardware_locked" && payload["reason"] == "timeout")
        {
            assert!(std::time::Instant::now() < deadline, "no auto-lock");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn last_lock(&self) -> Value {
        let events = self.recorder.0.lock().unwrap();
        events
            .iter()
            .rev()
            .find(|(event, _)| event == "hardware_locked")
            .map(|(_, payload)| payload.clone())
            .expect("no lock event")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self
            .runtime
            .block_on(ratspeak_runtime::shutdown_rns_lxmf(&self.state));
    }
}

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

// An auto-lock that fell due while the system slept fires within seconds of
// waking: the passcode identity locks, and its unlock goes with it.
#[test]
fn a_passcode_identity_locks_once_its_auto_lock_fell_due() {
    let _serial = serial();
    let f = Fixture::new();
    let a = f.unlocked_passcode_identity("60");
    let armed = f.armed().expect("the unlock armed no auto-lock");
    assert_eq!(
        (armed.kind, armed.timeout),
        ("passcode", Duration::from_secs(60))
    );
    // Unlocked an hour ago, before a long sleep.
    f.backdate(Duration::from_secs(3600));

    f.wait_until_locked(Duration::from_secs(15));

    assert_eq!(f.state.hw_locked_hash().as_deref(), Some(a.as_str()));
    assert!(f.loaded().is_none());
    let locked = f.last_lock();
    assert_eq!(locked["kind"], "passcode");
    assert_eq!(locked["reason"], "timeout");
    f.launch();
    assert_eq!(
        f.state.hw_locked_hash().as_deref(),
        Some(a.as_str()),
        "the unlock outlived the lock"
    );
    assert!(f.loaded().is_none());
}

// The lock needs no network: an identity whose network failed to start
// after it loaded still locks once its time is up.
#[test]
fn a_passcode_identity_locks_when_its_network_did_not_start() {
    let _serial = serial();
    let f = Fixture::new();
    let a = f.import("A");
    f.set_passcode(&a, PASSCODE, None);
    db::try_set_setting(&f.state.db, "hardware_session_timeout", "60").unwrap();
    db::set_active_identity(&f.state.db, &a).unwrap();
    f.launch();
    // The network's configuration becomes unreadable, so it can't start.
    std::fs::remove_file(f.rns_dir.join("config")).unwrap();
    std::fs::create_dir(f.rns_dir.join("config")).unwrap();
    f.unlock(PASSCODE);
    assert_eq!(f.loaded().as_deref(), Some(a.as_str()));
    assert_eq!(f.state.get_startup_stage(), "error");
    f.backdate(Duration::from_secs(3600));

    f.wait_until_locked(Duration::from_secs(15));

    assert_eq!(f.state.hw_locked_hash().as_deref(), Some(a.as_str()));
    assert!(f.loaded().is_none());
    assert_eq!(f.last_lock()["reason"], "timeout");
}

// Stopping the network leaves the identity active, and it still locks.
#[test]
fn a_passcode_identity_locks_after_its_network_was_stopped() {
    let _serial = serial();
    let f = Fixture::new();
    let a = f.unlocked_passcode_identity("60");
    f.runtime
        .block_on(ratspeak_runtime::shutdown_rns_lxmf(&f.state))
        .unwrap();
    assert!(f.loaded().is_none());
    f.backdate(Duration::from_secs(3600));

    f.wait_until_locked(Duration::from_secs(15));

    assert_eq!(f.state.hw_locked_hash().as_deref(), Some(a.as_str()));
    assert_eq!(f.last_lock()["reason"], "timeout");
    f.launch();
    assert_eq!(
        f.state.hw_locked_hash().as_deref(),
        Some(a.as_str()),
        "the unlock outlived the lock"
    );
    assert!(f.loaded().is_none());
}

// A due lock that cannot be carried out yet stays armed for the next check,
// instead of being dropped.
#[test]
fn a_due_auto_lock_that_cannot_be_carried_out_stays_armed() {
    let _serial = serial();
    let f = Fixture::new();
    let a = f.unlocked_passcode_identity("60");
    // From here on every session teardown is refused.
    f.runtime.block_on(f.state.activity.shutdown()).unwrap();
    f.backdate(Duration::from_secs(3600));

    // Longer than the at most 5 s between two checks of the deadline.
    std::thread::sleep(Duration::from_secs(8));

    assert!(f.state.hw_locked_hash().is_none());
    let armed = f.armed().expect("the due lock was dropped");
    assert_eq!(armed.hash, a);
}

// A fresh unlock starts the deadline over, whatever an earlier unlock of the
// same identity left armed.
#[test]
fn a_fresh_unlock_starts_the_auto_lock_over() {
    let _serial = serial();
    let f = Fixture::new();
    let a = f.unlocked_passcode_identity("3600");
    let b = f.import("B");
    f.set_passcode(&b, OTHER_PASSCODE, None);
    f.backdate(Duration::from_secs(1800));
    f.switch(&b);
    assert_eq!(f.state.hw_locked_hash().as_deref(), Some(b.as_str()));
    f.switch(&a);
    assert_eq!(f.state.hw_locked_hash().as_deref(), Some(a.as_str()));

    f.unlock(PASSCODE);

    let armed = f.armed().expect("the unlock armed no auto-lock");
    assert_eq!(armed.hash, a);
    assert!(
        ContinuousInstant::now().saturating_duration_since(armed.unlocked_at)
            < Duration::from_secs(60),
        "the unlock kept an earlier deadline"
    );
}

// Setting a passcode on the active identity starts its auto-lock, a new
// passcode starts it over, and removing the passcode ends it.
#[test]
fn a_passcode_change_on_the_active_identity_arms_or_ends_its_auto_lock() {
    let _serial = serial();
    let f = Fixture::new();
    let a = f.import("A");
    db::try_set_setting(&f.state.db, "hardware_session_timeout", "60").unwrap();
    db::set_active_identity(&f.state.db, &a).unwrap();
    f.launch();
    assert_eq!(f.loaded().as_deref(), Some(a.as_str()));
    assert!(f.armed().is_none(), "a plain identity has no auto-lock");

    f.set_passcode(&a, PASSCODE, None);
    let armed = f.armed().expect("the new passcode armed no auto-lock");
    assert_eq!(
        (armed.hash.as_str(), armed.kind, armed.timeout),
        (a.as_str(), "passcode", Duration::from_secs(60))
    );

    f.backdate(Duration::from_secs(30));
    f.set_passcode(&a, OTHER_PASSCODE, Some(PASSCODE));
    let armed = f.armed().expect("the changed passcode ended the auto-lock");
    assert!(
        ContinuousInstant::now().saturating_duration_since(armed.unlocked_at)
            < Duration::from_secs(20),
        "the changed passcode kept the earlier deadline"
    );

    f.remove_passcode(&a, OTHER_PASSCODE);
    assert!(
        f.armed().is_none(),
        "the removed passcode kept its auto-lock"
    );
    assert!(f.state.hw_locked_hash().is_none());
}
