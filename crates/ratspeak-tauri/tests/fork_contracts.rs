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
    for command in [
        "add_contact",
        "add_tcp_connection",
        "add_backbone_connection",
    ] {
        assert!(backup.contains(command), "import must use {command}");
    }

    // The announce radar and route lookup use data and commands the app already has.
    assert!(extensions.contains("announceRadarUpdate("));
    assert!(extensions.contains("announceRadarPulse("));
    assert!(!radar.contains("RS.invoke("));
    assert!(route.contains("'api_path_query'") && route.contains("'request_path'"));
    assert!(extensions.contains("openRouteSheet("));
}

/// Battleship is a fork-only game. It must stay out of upstream-owned files
/// except through the three one-line hooks, keep each fleet secret behind a
/// commitment, and keep the view thin (the Rust engine decides every outcome).
#[test]
fn battleship_is_a_hooked_fork_module_that_keeps_fleets_secret() {
    let engine = source("crates/ratspeak-runtime/src/battleship.rs");
    let production = engine.split("#[cfg(test)]").next().unwrap();
    let runtime_lib = source("crates/ratspeak-runtime/src/lib.rs");
    let state = source("crates/ratspeak-runtime/src/state.rs");
    let admission = source("crates/ratspeak-runtime/src/inbound_admission/game.rs");
    let runtime_manifest = source("crates/ratspeak-runtime/Cargo.toml");
    let hooks = source("fork/hooks.json");
    let view = source("dashboard/static/js/ru_battleship.js");
    let loader = source("dashboard/static/js/ru_loader.js");
    let css = source("dashboard/static/css/ratspeak-ru.css");
    let index = source("dashboard/index.html");

    // Hooks: the module is private (public API snapshot untouched), and the
    // game is registered where upstream builds a router, before hydration.
    for id in [
        "runtime-mod-battleship",
        "state-register-battleship",
        "admission-register-battleship",
    ] {
        assert!(hooks.contains(id), "missing hook {id}");
    }
    assert!(runtime_lib.contains("\nmod battleship;\n"));
    assert!(!runtime_lib.contains("pub mod battleship"));
    let router_line = state
        .find("let lrgp_router = lrgp::router::LrgpRouter::with_builtin_apps();")
        .expect("upstream router construction");
    let register_line = state
        .find("crate::battleship::register(&lrgp_router);")
        .expect("battleship registration hook");
    let restore_loop = state
        .find("lrgp_router.restore_session(session)")
        .expect("hydration loop");
    assert!(router_line < register_line && register_line < restore_loop);
    assert!(
        state.contains("#[cfg(not(test))]\n        crate::battleship::register(&lrgp_router);")
    );
    assert!(admission.contains("crate::battleship::register(&working);"));

    // No new dependency: SHA-256 comes from rns-crypto, so upstream's
    // manifests, lockfile and API baseline stay byte-identical.
    assert!(!runtime_manifest.contains("sha2"));
    assert!(production.contains("rns_crypto::sha::sha256(&input)"));
    assert!(production.contains("pub(crate) const APP_ID: &str = \"ru_battleship\";"));

    // Commitments bind the session and player; only the commit payload's
    // digest is sent before the reveal.
    assert!(production.contains("ratspeak.battleship.v1"));
    assert!(production.contains("input.extend_from_slice(session_id.as_bytes())"));
    assert!(production.contains("input.extend_from_slice(player.as_bytes())"));
    assert!(production.contains("(\"h\".into(), rmpv::Value::Binary(digest.to_vec()))"));
    let reveal_marker = "// Reveal: the loser's carries the final answer.";
    let (before_reveal, reveal_branch) = production.split_once(reveal_marker).expect("reveal");
    assert!(reveal_branch.contains("rmpv::Value::Binary(game.my_fleet.clone())"));
    assert!(!before_reveal.contains("Binary(game.my_salt"));
    assert!(!before_reveal.contains("Binary(game.my_fleet"));
    assert!(production.contains("fn exact_keys("));
    assert!(production.contains("fn rollback_session("));

    // The view only pre-fills intents and never touches salts or storage.
    assert!(view.contains("var APP_ID = 'ru_battleship';"));
    assert!(view.contains("RS.games.views.register(APP_ID"));
    for forbidden in ["my_salt", "localStorage", "sessionStorage"] {
        assert!(!view.contains(forbidden), "view must not use {forbidden}");
    }
    assert!(view.contains("context.sendMove({ k: 'c', p: fleet }"));
    assert!(view.contains("context.sendMove({ k: 'f', c: cell }"));
    assert!(view.contains("context.sendMove({ k: 'r' })"));

    // Loaded only through the fork loader; upstream's index.html is not edited
    // for it and its stylesheet stays untouched.
    assert!(loader.contains("'/static/js/ru_battleship.js'"));
    assert!(!index.contains("ru_battleship"));
    assert!(css.contains(".games-theme-ru_battleship"));
    let upstream_css = source("dashboard/static/css/11-games.css");
    assert!(!upstream_css.contains("bs-cell"));
}
