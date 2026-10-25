//! Identity data commands over the serialized IPC boundary: what they refuse
//! with no active identity, and what bulk deletes keep and publish. Identity
//! writes bump a process-global generation, so these tests run one at a time.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use r2d2_sqlite::SqliteConnectionManager;
use ratspeak_core::EmitError;
use ratspeak_tauri::{config::DashboardConfig, db, state::AppState};
use serde_json::{Value, json};
use tauri::test::{MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PEER: &str = "cccccccccccccccccccccccccccccccc";
const SPAM: &str = "dddddddddddddddddddddddddddddddd";

static SERIAL: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<(String, Value)>>,
}

impl ratspeak_core::Emitter for Recorder {
    fn try_emit(&self, event: &str, payload: Value) -> Result<(), EmitError> {
        self.events
            .lock()
            .unwrap()
            .push((event.to_owned(), payload));
        Ok(())
    }
}

struct Fixture {
    window: tauri::WebviewWindow<MockRuntime>,
    app: tauri::App<MockRuntime>,
    state: Arc<AppState>,
    recorder: Arc<Recorder>,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn without_active_identity(gated: bool) -> Self {
        Self::new(None, gated)
    }

    fn new(active: Option<&str>, gated: bool) -> Self {
        Self::with_pool(active, gated, |root| db::init_pool(root).unwrap())
    }

    /// One pooled connection: while the test holds it, every read fails.
    fn single_connection(active: Option<&str>) -> Self {
        Self::with_pool(active, false, |root| {
            let manager = SqliteConnectionManager::file(root.join(".ratspeak/ratspeak.db"))
                .with_init(|conn| {
                    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=30000;")
                });
            r2d2::Pool::builder()
                .max_size(1)
                .connection_timeout(std::time::Duration::from_millis(200))
                .build(manager)
                .unwrap()
        })
    }

    /// A and B each hold a contact, a block, a pending network block and a
    /// message with a stored file. `gated` puts the app's invoke gate in front.
    fn with_pool(
        active: Option<&str>,
        gated: bool,
        pool: impl FnOnce(&std::path::Path) -> db::DbPool,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        ratspeak_tauri::lxmf::initialize_storage_namespace(dir.path()).unwrap();
        let config = DashboardConfig::from_env_and_defaults(dir.path().to_path_buf());
        let pool = pool(dir.path());
        db::init_schema(&pool).unwrap();
        for identity in [A, B] {
            db::save_identity(&pool, identity, &identity[..16], "", "");
            db::save_contact(&pool, PEER, Some("Peer"), "trusted", identity);
            db::block_contact(&pool, SPAM, "Spam", identity);
            assert!(db::enqueue_pending_blackhole(
                &pool, SPAM, identity, None, None
            ));
            let stored = format!("{identity}.bin");
            db::try_save_message(
                &pool,
                &format!("{identity}-message"),
                PEER,
                identity,
                "hello",
                "",
                1.0,
                "received",
                "inbound",
                identity,
                "note.bin",
                &stored,
                "",
                "",
                "",
                "",
                None,
            )
            .unwrap();
            let files = media_dir(&config, identity);
            std::fs::create_dir_all(&files).unwrap();
            std::fs::write(files.join(&stored), b"media").unwrap();
        }
        if let Some(active) = active {
            db::set_active_identity(&pool, active).unwrap();
        }
        let recorder = Arc::new(Recorder::default());
        let state = Arc::new(AppState::new(
            config,
            pool,
            Arc::clone(&recorder) as Arc<dyn ratspeak_core::Emitter>,
            Arc::new(ratspeak_core::NoopNotifier),
        ));
        let builder = mock_builder().manage(Arc::clone(&state));
        let builder = if gated {
            builder.invoke_handler(ratspeak_tauri::invoke_gate::gated(commands()))
        } else {
            builder.invoke_handler(commands())
        };
        let app = builder.build(mock_context(noop_assets())).unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        Self {
            window,
            app,
            state,
            recorder,
            _dir: dir,
        }
    }

    fn last_event(&self, name: &str) -> Option<Value> {
        let events = self.recorder.events.lock().unwrap();
        events
            .iter()
            .rev()
            .find(|(event, _)| event == name)
            .map(|(_, payload)| payload.clone())
    }

    fn wait_for_event(&self, name: &str) -> Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(payload) = self.last_event(name) {
                return payload;
            }
            assert!(std::time::Instant::now() < deadline, "no {name}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn invoke(&self, command: &str, body: Value) -> Result<Value, Value> {
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
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
        )
        .map(|response| response.deserialize().unwrap())
    }

    fn assert_every_identity_intact(&self) {
        let pool = &self.state.db;
        for identity in [A, B] {
            assert_eq!(db::get_all_contacts(pool, identity).len(), 1, "{identity}");
            assert_blocks_kept(pool, identity);
            assert_eq!(
                db::get_conversation(pool, PEER, identity, 10).len(),
                1,
                "{identity} messages"
            );
            let stored = media_dir(&self.state.config, identity).join(format!("{identity}.bin"));
            assert!(stored.is_file(), "{identity} media");
        }
        let tombstones: i64 = pool
            .get()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM lxmf_user_deleted", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(tombstones, 0);
    }
}

fn commands() -> impl Fn(tauri::ipc::Invoke<MockRuntime>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        ratspeak_tauri::commands::system::api_version,
        ratspeak_tauri::commands::system::api_setup_complete,
        ratspeak_tauri::commands::system::api_clear_messages,
        ratspeak_tauri::commands::system::api_clear_contacts,
        ratspeak_tauri::commands::system::api_reset_database,
        ratspeak_tauri::commands::messaging::delete_conversation,
        ratspeak_tauri::commands::contacts::api_contacts,
        ratspeak_tauri::commands::contacts::add_contact,
        ratspeak_tauri::commands::contacts::block_contact,
        ratspeak_tauri::commands::messaging::hide_conversation,
        ratspeak_tauri::commands::messaging::send_lxmf_message,
        ratspeak_tauri::commands::messaging::send_reaction,
        ratspeak_tauri::commands::messaging::send_lxmf_reply,
        ratspeak_tauri::commands::messaging::send_lxmf_with_attachment,
        ratspeak_tauri::commands::contact_card::import_contact_card,
    ]
}

fn media_dir(config: &DashboardConfig, identity: &str) -> PathBuf {
    config
        .data_dir
        .join("identities")
        .join(identity)
        .join("files")
}

fn identity_dirs(state: &AppState) -> Vec<String> {
    let mut dirs: Vec<String> = std::fs::read_dir(state.config.data_dir.join("identities"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    dirs.sort();
    dirs
}

fn active_hash(pool: &db::DbPool) -> Option<String> {
    pool.get()
        .unwrap()
        .query_row(
            "SELECT hash FROM identities WHERE is_active = 1",
            [],
            |row| row.get(0),
        )
        .ok()
}

fn assert_blocks_kept(pool: &db::DbPool, identity: &str) {
    assert!(db::is_blocked(pool, SPAM, identity), "{identity} block");
    assert_eq!(
        db::list_pending_blackholes_for_identity(pool, identity).len(),
        1,
        "{identity} pending network block"
    );
}

fn assert_no_active_identity(result: Result<Value, Value>) {
    let error = result.expect_err("refused");
    assert_eq!(error["code"], "conflict");
    assert_eq!(error["message"], "no active identity");
}

#[test]
fn bulk_deletes_refuse_when_no_identity_is_active() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (command, body) in [
        ("api_clear_messages", json!({})),
        ("api_clear_contacts", json!({})),
        ("api_reset_database", json!({})),
        ("delete_conversation", json!({ "hash": PEER })),
    ] {
        let fixture = Fixture::without_active_identity(false);
        let result = fixture.invoke(command, body);
        fixture.assert_every_identity_intact();
        assert_no_active_identity(result);
    }
}

#[test]
fn the_invoke_gate_refuses_identity_scoped_commands_without_an_active_identity() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = Fixture::without_active_identity(true);
    // Unknown until a read; the gate never reads the database itself.
    assert_eq!(fixture.invoke("api_contacts", json!({})), Ok(json!([])));
    assert_no_active_identity(fixture.invoke("api_contacts", json!({})));
    assert_no_active_identity(fixture.invoke(
        "add_contact",
        json!({ "args": { "hash": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" } }),
    ));
    assert!(db::get_all_contacts(&fixture.state.db, "").is_empty());
    assert!(fixture.invoke("api_version", json!({})).is_ok());

    db::set_active_identity(&fixture.state.db, A).unwrap();
    for _ in 0..2 {
        let contacts = fixture.invoke("api_contacts", json!({})).unwrap();
        assert_eq!(contacts.as_array().map(Vec::len), Some(1));
    }
}

#[test]
fn clearing_contacts_or_all_data_keeps_blocks() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for command in ["api_clear_contacts", "api_reset_database"] {
        let fixture = Fixture::new(Some(A), false);
        fixture.invoke(command, json!({})).unwrap();
        let pool = &fixture.state.db;
        assert!(db::get_all_contacts(pool, A).is_empty(), "{command}");
        assert_blocks_kept(pool, A);
        assert_eq!(db::get_all_contacts(pool, B).len(), 1);
        assert_blocks_kept(pool, B);
    }
}

#[test]
fn identity_rows_are_never_written_without_an_active_identity() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = Fixture::without_active_identity(false);
    let peer = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    for (command, body) in [
        ("add_contact", json!({ "args": { "hash": peer } })),
        (
            "block_contact",
            json!({ "args": { "hash": peer, "escalate_to_blackhole": true } }),
        ),
        ("hide_conversation", json!({ "hash": peer })),
    ] {
        let result = fixture.invoke(command, body);
        let conn = fixture.state.db.get().unwrap();
        for table in [
            "contacts",
            "blocked_contacts",
            "pending_blackholes",
            "hidden_conversations",
        ] {
            let rows: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE identity_id = ''"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(rows, 0, "{command} wrote an identity-less {table} row");
        }
        assert_no_active_identity(result);
    }
}

#[test]
fn bulk_deletes_publish_the_lists_they_changed() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (command, cleared, contacts, unread, conversations) in [
        ("api_clear_messages", (true, false), 1, 0, 0),
        ("api_clear_contacts", (false, true), 0, 1, 1),
        ("api_reset_database", (true, true), 0, 0, 0),
    ] {
        let fixture = Fixture::new(Some(A), false);
        fixture.invoke(command, json!({})).unwrap();
        // The window resets on this event, so it must precede the lists.
        let order: Vec<String> = fixture
            .recorder
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|(event, _)| event.clone())
            .filter(|event| event == "identity_data_cleared" || event == "contacts_update")
            .collect();
        assert_eq!(
            order,
            ["identity_data_cleared", "contacts_update"],
            "{command}"
        );
        assert_eq!(
            fixture.last_event("identity_data_cleared"),
            Some(json!({ "messages": cleared.0, "contacts": cleared.1 })),
            "{command}"
        );
        let listed = fixture.last_event("contacts_update").expect(command);
        assert_eq!(listed.as_array().map(Vec::len), Some(contacts), "{command}");
        assert_eq!(
            fixture.last_event("unread_total"),
            Some(json!({ "count": unread })),
            "{command}"
        );
        let listed = fixture.wait_for_event("conversations_update");
        assert_eq!(
            listed.as_array().map(Vec::len),
            Some(conversations),
            "{command}"
        );
    }
}

#[test]
fn setup_refuses_when_the_active_identity_cannot_be_read() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = Fixture::single_connection(Some(A));
    let boundary = fixture.state.current_activity_boundary_generation();
    let dirs = identity_dirs(&fixture.state);
    let held = fixture.state.db.get().unwrap();
    let result = fixture.invoke("api_setup_complete", json!({ "args": {} }));
    drop(held);
    assert_eq!(
        identity_dirs(&fixture.state),
        dirs,
        "no identity was created"
    );
    assert_eq!(db::get_all_identities(&fixture.state.db).len(), 2);
    assert_eq!(active_hash(&fixture.state.db).as_deref(), Some(A));
    assert_eq!(
        fixture.state.current_activity_boundary_generation(),
        boundary,
        "nothing was shut down"
    );
    assert_eq!(result.unwrap_err()["code"], "database_unavailable");
}

fn contact_card_payload() -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
    let identity = rns_identity::identity::Identity::new();
    let lxmf = rns_identity::destination::Destination::hash_from_name_and_identity(
        ratspeak_core::LXMF_DELIVERY_APP_NAME,
        Some(&identity.hash),
    );
    format!(
        "RSCP1:{}:{}:{}:{}",
        B64URL.encode("Card"),
        hex::encode(lxmf),
        hex::encode(identity.hash),
        B64URL.encode(identity.get_public_key()),
    )
}

#[test]
fn sends_and_card_imports_refuse_without_an_active_identity() {
    use base64::Engine;
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = Fixture::without_active_identity(false);
    for (command, body) in [
        (
            "send_lxmf_message",
            json!({ "args": { "dest_hash": PEER, "content": "hi" } }),
        ),
        (
            "send_reaction",
            json!({ "args": { "dest_hash": PEER, "message_id": "m1", "emoji": "+" } }),
        ),
        (
            "send_lxmf_reply",
            json!({ "args": { "dest_hash": PEER, "content": "hi", "reply_to_id": "m1" } }),
        ),
        (
            "send_lxmf_with_attachment",
            json!({ "args": {
                "dest_hash": PEER,
                "file_data": base64::engine::general_purpose::STANDARD.encode(b"file"),
                "file_name": "note.txt",
            } }),
        ),
        (
            "import_contact_card",
            json!({ "payload": contact_card_payload() }),
        ),
    ] {
        let result = fixture.invoke(command, body);
        let conn = fixture.state.db.get().unwrap();
        for table in ["messages", "contacts", "reactions"] {
            let rows: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE identity_id = ''"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(rows, 0, "{command} wrote an identity-less {table} row");
        }
        assert_eq!(
            result,
            Err(json!({ "code": "conflict", "message": "no active identity" })),
            "{command}"
        );
    }
}

#[test]
fn a_bulk_delete_that_waited_out_a_switch_deletes_nothing() {
    use std::future::{Future, poll_fn};
    use std::pin::Pin;
    use std::task::Poll;
    use tauri::Manager;
    type Delete<'a> =
        Pin<Box<dyn Future<Output = ratspeak_tauri::error::AppResult<Value>> + Send + 'a>>;
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for command in [
        "api_clear_messages",
        "api_clear_contacts",
        "api_reset_database",
        "delete_conversation",
    ] {
        let fixture = Fixture::new(Some(A), false);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async {
            let managed = fixture.app.state::<Arc<AppState>>();
            let switch = fixture.state.identity_switch_lock.lock().await;
            let mut delete: Delete<'_> = match command {
                "api_clear_messages" => Box::pin(
                    ratspeak_tauri::commands::system::api_clear_messages(managed),
                ),
                "api_clear_contacts" => Box::pin(
                    ratspeak_tauri::commands::system::api_clear_contacts(managed),
                ),
                "api_reset_database" => Box::pin(
                    ratspeak_tauri::commands::system::api_reset_database(managed),
                ),
                _ => Box::pin(ratspeak_tauri::commands::messaging::delete_conversation(
                    managed,
                    PEER.to_string(),
                )),
            };
            // The user asked while A was active; the delete now waits for the lock.
            poll_fn(|cx| {
                assert!(delete.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            db::set_active_identity(&fixture.state.db, B).unwrap();
            fixture.state.bump_identity_session_generation();
            drop(switch);
            delete.await
        });
        fixture.assert_every_identity_intact();
        assert_eq!(result.unwrap_err().code, "conflict", "{command}");
    }
}
