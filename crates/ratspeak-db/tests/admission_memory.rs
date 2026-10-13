//! Run as its own test executable: SQLite status counters are process-global.
//! This qualifies design headroom; it is not a process RSS or allocator limit.
use ratspeak_db::admission::{self, DurableInboundStore, EffectCommit, PreparedAdmission};
use rusqlite::{ffi, params};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountedRust;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn add(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for CountedRust {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            add(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(ptr, old, size) };
        if !next.is_null() {
            LIVE.fetch_sub(old.size(), Ordering::Relaxed);
            add(size);
        }
        next
    }
}
#[global_allocator]
static ALLOC: CountedRust = CountedRust;
fn native_status(reset: bool) -> (i64, i64) {
    let (mut current, mut high) = (0, 0);
    assert_eq!(
        unsafe {
            ffi::sqlite3_status64(
                ffi::SQLITE_STATUS_MEMORY_USED,
                &mut current,
                &mut high,
                i32::from(reset),
            )
        },
        ffi::SQLITE_OK,
        "SQLite memory accounting unavailable"
    );
    (current, high)
}
fn connection_status(conn: &rusqlite::Connection, op: i32) -> (i32, i32) {
    let (mut current, mut high) = (0, 0);
    assert_eq!(
        unsafe { ffi::sqlite3_db_status(conn.handle(), op, &mut current, &mut high, 0) },
        ffi::SQLITE_OK
    );
    (current, high)
}
#[test]
fn maximal_admission_plans_fit_qualified_sqlite_headroom() {
    let root = std::env::temp_dir().join(format!(
        "ratspeak-admission-native-memory-{}",
        std::process::id()
    ));
    assert!(
        !root.exists(),
        "previous qualification evidence must be reviewed first"
    );
    std::fs::create_dir_all(&root).unwrap();
    let pool = ratspeak_db::init_pool(&root).unwrap();
    ratspeak_db::init_schema(&pool).unwrap();
    let store = DurableInboundStore::from_pool(&pool).unwrap();
    let mut warm = pool.get().unwrap();
    let before = native_status(true).0;
    let known: Vec<u8> = warm
        .query_row("SELECT zeroblob(1048576)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(known.len(), 1_048_576);
    assert!(
        native_status(false).1 > before + 500_000,
        "SQLite instrumentation did not observe a known allocation"
    );
    drop(known);
    {
        let tx = warm.transaction().unwrap();
        let mut insert=tx.prepare("INSERT INTO messages(id,source,destination,content,title,timestamp,state,direction,identity_id) VALUES(?1,'peer','local',?2,'',1,'received','inbound',?3)").unwrap();
        for n in 0..20_000 {
            let content = (0..48)
                .map(|word| format!("token{n}word{word} "))
                .collect::<String>();
            insert
                .execute(params![format!("warm-{n}"), content, "11".repeat(16)])
                .unwrap();
        }
        drop(insert);
        tx.commit().unwrap();
    }
    warm.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    // Leave room for fixed receipt/lookups and every other binding occurrence.
    let available = admission::MAX_PROJECTION_BINDINGS - 4096 - "archive".len() - 64 - 32 - 8 - 5 - 8 - 7;
    let unique = (0..)
        .map(|i| format!("unique{i:09} "))
        .take(available / 16 + 1)
        .collect::<String>();
    let mut lossy = String::from_utf8_lossy(&vec![0xff; available / 3]).into_owned();
    lossy.push_str(&"x".repeat(available-lossy.len()));
    let metadata = admission::bounded_json(
        &serde_json::json!({"attachment":"a".repeat(available-100),"reaction":"x","game":{}}),
        available,
    )
    .unwrap();
    let cases = [
        (
            "ordinary",
            ("ordinary message ".repeat(available / 17 + 1))[..available].to_owned(),
            String::new(),
        ),
        (
            "unique_tokens",
            unique[..available.min(unique.len())].to_owned(),
            String::new(),
        ),
        (
            "repeated_tokens",
            ("word ".repeat(available / 5 + 1))[..available].to_owned(),
            String::new(),
        ),
        ("lossy_utf8", lossy, String::new()),
        ("metadata", metadata, String::new()),
        (
            "duplicate_bindings",
            "x".repeat(available / 2),
            "x".repeat(available - available / 2),
        ),
    ];
    for (index, (name, body, title)) in cases.into_iter().enumerate() {
        let id = [index as u8 + 1; 32];
        let id_hex = hex::encode(id);
        let identity = "11".repeat(16);
        let plan = PreparedAdmission::new(
            identity.clone(),
            id,
            [2; 16],
            id,
            id,
            "archive".into(),
            1000,
            [
                body.len(),
                title.len(),
                id_hex.len(),
                identity.len(),
                8,
                5,
                8,
                7,
            ],
        )
        .unwrap();
        let baseline = native_status(true).0;
        let rust_baseline = LIVE.load(Ordering::Relaxed);
        PEAK.store(rust_baseline, Ordering::Relaxed);
        let receipt=store.commit(admission::try_reserve_admission().unwrap(),plan,|_,_,_|Ok(()),|conn| {
            conn.execute("INSERT INTO messages(id,source,destination,content,title,timestamp,state,direction,identity_id) VALUES(?1,'peer','local',?2,?3,1,'received','inbound',?4)",params![id_hex,body,title,identity])?;
            let cache=connection_status(conn,ffi::SQLITE_DBSTATUS_CACHE_USED);
            let lookaside=connection_status(conn,ffi::SQLITE_DBSTATUS_LOOKASIDE_USED);
            eprintln!("case={name} cache_bytes={} lookaside_current={} lookaside_peak={}",cache.0,lookaside.0,lookaside.1);
            Ok(EffectCommit::Applied("message"))
        }).unwrap();
        assert_eq!(receipt.message_id, id);
        let native_growth = native_status(false).1.saturating_sub(baseline);
        let rust_growth = PEAK.load(Ordering::Relaxed).saturating_sub(rust_baseline);
        eprintln!(
            "case={name} sqlite_version={} native_peak_delta={native_growth} rust_peak_delta={rust_growth}",
            rusqlite::version()
        );
        assert!(
            native_growth <= admission::NATIVE_DB_HEADROOM as i64,
            "{name}: native SQLite headroom exceeded"
        );
    }
    drop(warm);
    drop(pool);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
