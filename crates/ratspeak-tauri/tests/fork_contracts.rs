//! Contracts for the Ratspeak-ru fork additions. They live in their own file so
//! that merging upstream never touches `source_contracts.rs`.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

fn read_source(path: impl AsRef<Path>) -> std::io::Result<String> {
    fs::read_to_string(path).map(|source| source.replace("\r\n", "\n").replace('\r', "\n"))
}

fn source(relative: &str) -> String {
    read_source(repo_root().join(relative)).unwrap_or_else(|_| panic!("read {relative}"))
}

#[test]
fn native_connection_links_prefill_forms_and_never_connect_on_their_own() {
    let native = source("src-tauri/src/connection_deep_link.rs");
    let parser = source("crates/ratspeak-tauri/src/commands/connection_link.rs");
    let lib = source("src-tauri/src/lib.rs");
    let manifest = source("src-tauri/gen/android/app/src/main/AndroidManifest.xml");
    let bridge = source("dashboard/static/js/native_connection_link.js");
    let presenters = source("dashboard/static/js/connection_link_presenters.js");
    let text_share = source("src-tauri/src/text_share/android.rs");

    // Rust is the only parser; the frontend never sees the raw platform URL.
    assert!(native.contains("parse_connection_link(payload)"));
    assert!(native.contains("app.deep_link().on_open_url"));
    assert!(bridge.contains("RS.invoke('peek_native_connection_link')"));
    assert!(bridge.contains("RS.invoke('ack_native_connection_link',"));
    assert!(bridge.contains("'native_connection_link_available'"));
    assert!(!bridge.contains("on_open_url"));
    assert!(native.contains("app.manage(NativeConnectionLinkInbox::default())"));
    assert!(lib.contains("connection_deep_link::peek_native_connection_link"));
    assert!(lib.contains("connection_deep_link::ack_native_connection_link"));
    assert!(lib.contains("connection_deep_link::validate_connection_link"));
    assert!(lib.contains("connection_deep_link::install(app)"));
    assert!(lib.contains("connection_deep_link::extend_android_runtime(context.config_mut())"));
    assert!(text_share.contains("connection_deep_link::accept_shared_text"));

    // A link pre-fills the normal forms. It must not call an add command.
    for forbidden in [
        "add_tcp_connection",
        "add_lora_interface",
        "submitConnection(",
        "rnodeSubmit",
    ] {
        assert!(
            !presenters.contains(forbidden),
            "connection links must not call {forbidden}"
        );
    }

    // The parser rejects config-injection characters; radio links carry no device
    // field (unit-tested in connection_link.rs).
    assert!(parser.contains("'[' | ']' | '=' | '#'"));
    assert!(parser.contains("c.is_control() || c == '#'"));
    assert!(manifest.contains(r#"android:host="tcp""#));
    assert!(manifest.contains(r#"android:host="radio""#));
}

#[test]
fn fork_frontend_is_loaded_by_one_hook_and_only_offers_validated_actions() {
    let index = source("dashboard/index.html");
    let loader = source("dashboard/static/js/ru_loader.js");
    let extensions = source("dashboard/static/js/ru_extensions.js");
    let qr = source("dashboard/static/js/connection_qr.js");
    let backup = source("dashboard/static/js/contacts_backup.js");
    let radar = source("dashboard/static/js/announce_radar.js");
    let route = source("dashboard/static/js/route_lookup.js");

    // One line in index.html loads everything the fork adds.
    assert!(index.contains("/static/js/ru_loader.js"));
    for script in [
        "connection_link_presenters.js",
        "native_connection_link.js",
        "connection_qr.js",
        "contacts_backup.js",
        "announce_radar.js",
        "route_lookup.js",
        "ru_extensions.js",
    ] {
        assert!(loader.contains(script), "loader must load {script}");
    }
    assert!(loader.contains("settings-export-contacts-btn"));
    assert!(loader.contains("settings-import-contacts-btn"));
    assert!(loader.contains("announce-radar-canvas"));

    // "Share QR code" only offers links that Rust itself validated.
    assert!(qr.contains("RS.invoke('validate_connection_link'"));
    assert!(extensions.contains("openConnectionQrShare(ifaceType, ifaceName)"));

    // Contacts and connections backup only goes through the normal add commands.
    for command in ["add_contact", "add_tcp_connection", "add_backbone_connection"] {
        assert!(backup.contains(command), "import must use {command}");
    }

    // The announce radar and route lookup use data and commands the app already has.
    assert!(extensions.contains("announceRadarUpdate("));
    assert!(extensions.contains("announceRadarPulse("));
    assert!(!radar.contains("RS.invoke("));
    assert!(route.contains("'api_path_query'") && route.contains("'request_path'"));
    assert!(extensions.contains("openRouteSheet("));
}
