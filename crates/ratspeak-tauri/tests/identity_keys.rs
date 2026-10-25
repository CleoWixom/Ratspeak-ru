//! Releasing what controls an identity: through the IPC surface and its
//! invoke gate, as the window asks for it. The vault runs one job per process,
//! so these tests take turns.

use std::sync::{Arc, Mutex, MutexGuard};

use ratspeak_tauri::{config::DashboardConfig, db, state::AppState};
use serde_json::{Value, json};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};

const PASSCODE: &str = "correct horse";

static VAULT: Mutex<()> = Mutex::new(());

struct Fixture {
    state: Arc<AppState>,
    window: tauri::WebviewWindow<MockRuntime>,
    _app: tauri::App<MockRuntime>,
    _root: tempfile::TempDir,
    _vault: MutexGuard<'static, ()>,
}

impl Fixture {
    fn new() -> Self {
        let vault = VAULT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir().unwrap();
        let data_root = root.path().to_path_buf();
        ratspeak_runtime::lxmf::initialize_storage_namespace(&data_root).unwrap();
        let config = DashboardConfig {
            data_root: data_root.clone(),
            data_dir: data_root.join(".ratspeak"),
            rns_config_dir: data_root.join("rns"),
            rns_config_dir_overridden: true,
            max_log_entries: 20,
        };
        let pool = db::init_pool(&data_root).unwrap();
        db::init_schema(&pool).unwrap();
        let state = Arc::new(AppState::new(
            config,
            pool,
            Arc::new(ratspeak_core::NoopEmitter),
            Arc::new(ratspeak_core::NoopNotifier),
        ));
        let app = mock_builder()
            .manage(Arc::clone(&state))
            .invoke_handler(ratspeak_tauri::invoke_gate::gated(
                tauri::generate_handler![
                    ratspeak_tauri::commands::identity::set_identity_passcode,
                    ratspeak_tauri::commands::network_ownership::export_shared_access,
                ],
            ))
            .build(mock_context(noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        Self {
            state,
            window,
            _app: app,
            _root: root,
            _vault: vault,
        }
    }

    fn call(&self, command: &str, args: Value) -> Result<Value, Value> {
        get_ipc_response(
            &self.window,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: if cfg!(any(windows, target_os = "android")) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
        .map(|response| response.deserialize().unwrap())
    }

    /// A plain identity on disk and in the identity table, made the active one.
    fn activate_new(&self, nickname: &str) -> String {
        let key = rns_identity::identity::Identity::new()
            .get_private_key()
            .unwrap()
            .to_vec();
        let (hash, _) = ratspeak_runtime::lxmf::LxmfManager::import_identity_to_data_dir(
            &self.state.config.data_dir,
            &key,
            nickname,
            &self.state.db,
        )
        .unwrap();
        db::set_active_identity(&self.state.db, &hash).unwrap();
        hash
    }

    fn protect(&self, identity: &str) {
        self.call(
            "set_identity_passcode",
            json!({ "args": { "hash": identity, "passcode": PASSCODE } }),
        )
        .unwrap();
    }
}

/// Refused before the network is asked: the export's authorization failed.
fn refused(result: Result<Value, Value>) -> Value {
    let error = result.expect_err("released without authorization");
    assert_ne!(error["message"], "Network is not running", "{error}");
    error
}

/// Authorized: the export goes on to the network, which these tests don't run.
fn authorized(result: Result<Value, Value>) {
    let error = result.expect_err("no network runs here");
    assert_eq!(error["message"], "Network is not running", "{error}");
}

// The shared instance's control key is an admin credential: copying it takes
// the active identity's PIN, or a confirmation when it has none.
#[test]
fn the_shared_instance_key_needs_the_pin_or_a_confirmation() {
    let f = Fixture::new();
    db::set_setting(&f.state.db, "developer_mode_enabled", "true");

    let protected = f.activate_new("Protected");
    f.protect(&protected);
    assert_eq!(
        refused(f.call("export_shared_access", json!({})))["code"],
        "unauthorized"
    );
    assert_eq!(
        refused(f.call(
            "export_shared_access",
            json!({ "passcode": "wrong horse", "confirmed": true })
        ))["code"],
        "unauthorized"
    );
    authorized(f.call("export_shared_access", json!({ "passcode": PASSCODE })));

    f.activate_new("Plain");
    refused(f.call("export_shared_access", json!({})));
    refused(f.call("export_shared_access", json!({ "confirmed": false })));
    authorized(f.call("export_shared_access", json!({ "confirmed": true })));
}
