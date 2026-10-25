//! Native `ratspeak://tcp` and `ratspeak://radio` lifecycle integration.
//!
//! Mirrors `channel_deep_link`: platform-delivered URLs cross the canonical
//! Rust parser once, and only the typed target is retained in a bounded
//! process-memory inbox until application JavaScript acknowledges that it
//! presented the pre-filled form. A link never creates an interface by itself;
//! the user always confirms in the normal add-connection UI.

use std::sync::{Mutex, MutexGuard};

use ratspeak_tauri::commands::connection_link::{ConnectionLinkTarget, parse_connection_link};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_deep_link::DeepLinkExt;
use url::Url;

const NATIVE_CONNECTION_LINK_AVAILABLE: &str = "native_connection_link_available";

/// Android's plugin filters incoming Intents independently of the manifest.
/// Append the connection hosts to the filter `channel_deep_link` installs; the
/// manifest declares the same hosts statically.
#[cfg(any(target_os = "android", test))]
pub(crate) fn extend_android_runtime(config: &mut tauri::Config) {
    let entry = config
        .plugins
        .0
        .entry("deep-link".into())
        .or_insert_with(|| serde_json::json!({ "mobile": [] }));
    let Some(object) = entry.as_object_mut() else {
        return;
    };
    let mobile = object
        .entry("mobile")
        .or_insert_with(|| serde_json::json!([]));
    if let Some(list) = mobile.as_array_mut() {
        for host in ["tcp", "radio"] {
            list.push(serde_json::json!({ "scheme": ["ratspeak"], "host": host }));
        }
    }
}

#[derive(Default)]
pub(crate) struct NativeConnectionLinkInbox {
    state: Mutex<InboxState>,
}

#[derive(Default)]
struct InboxState {
    last_revision: u64,
    pending: Option<PendingLink>,
}

struct PendingLink {
    revision: u64,
    target: ConnectionLinkTarget,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeConnectionLinkDelivery {
    // JavaScript numbers cannot represent every u64 exactly. This token is
    // opaque to the frontend and echoed byte-for-byte in the acknowledgment.
    revision: String,
    activity_generation: Option<String>,
    target: ConnectionLinkTarget,
}

impl NativeConnectionLinkInbox {
    fn state(&self) -> MutexGuard<'_, InboxState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Accept every valid link in arrival order and retain only the newest.
    /// Returns `true` when the observable pending value changed.
    fn accept_payloads<'a>(&self, payloads: impl IntoIterator<Item = &'a str>) -> bool {
        let mut latest = None;
        let mut rejected = 0usize;
        for payload in payloads {
            // `ratspeak://channel` URLs share the scheme and belong to the
            // channel inbox; they are not an error here.
            if payload.starts_with("ratspeak://channel") {
                continue;
            }
            match parse_connection_link(payload) {
                Ok(target) => latest = Some(target),
                Err(_) => rejected = rejected.saturating_add(1),
            }
        }
        if rejected > 0 {
            tracing::debug!(
                rejected,
                reason = "invalid_native_connection_link",
                "ignored non-canonical native connection link"
            );
        }

        let Some(latest) = latest else {
            return false;
        };
        let mut state = self.state();
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.target == latest)
        {
            return false;
        }
        // Never reuse a delivery token, even at exhaustion.
        let Some(revision) = state.last_revision.checked_add(1) else {
            return false;
        };
        state.last_revision = revision;
        state.pending = Some(PendingLink {
            revision,
            target: latest,
        });
        tracing::debug!(
            reason = "native_connection_link_accepted",
            "accepted a native connection link"
        );
        true
    }

    fn peek(&self) -> Option<NativeConnectionLinkDelivery> {
        let state = self.state();
        tracing::debug!(
            pending_present = state.pending.is_some(),
            reason = "native_connection_link_peeked",
            "inspected the pending native connection link"
        );
        state
            .pending
            .as_ref()
            .map(|pending| NativeConnectionLinkDelivery {
                revision: pending.revision.to_string(),
                activity_generation: None,
                target: pending.target.clone(),
            })
    }

    fn acknowledge(&self, revision: &str) -> bool {
        let mut state = self.state();
        // Exact string equality also rejects non-canonical decimal tokens.
        let accepted = state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.revision.to_string() == revision);
        if accepted {
            state.pending = None;
        }
        accepted
    }
}

fn enqueue_native_connection_links<R: Runtime>(app: &AppHandle<R>, urls: Vec<Url>) {
    let inbox = app.state::<NativeConnectionLinkInbox>();
    if inbox.accept_payloads(urls.iter().map(Url::as_str))
        && app.emit(NATIVE_CONNECTION_LINK_AVAILABLE, ()).is_err()
    {
        tracing::debug!(
            reason = "native_connection_link_event_unavailable",
            "could not notify the WebView about a native connection link"
        );
    }
}

/// Re-signal a link whose peek was blocked by Android's native lifecycle.
#[cfg(target_os = "android")]
pub(crate) fn notify_pending(app: &AppHandle) {
    let pending = app
        .state::<NativeConnectionLinkInbox>()
        .state()
        .pending
        .is_some();
    if pending {
        let _ = app.emit(NATIVE_CONNECTION_LINK_AVAILABLE, ());
    }
}

/// Register the running-app listener before sampling the cold-start value.
pub(crate) fn install(app: &mut tauri::App) {
    let listener_app = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        enqueue_native_connection_links(&listener_app, event.urls());
    });

    match app.deep_link().get_current() {
        Ok(Some(urls)) => enqueue_native_connection_links(app.handle(), urls),
        Ok(None) => {}
        Err(_) => tracing::debug!(
            reason = "native_connection_link_cold_start_unavailable",
            "could not inspect the native connection-link launch target"
        ),
    }
}

#[tauri::command]
pub(crate) fn peek_native_connection_link(
    inbox: State<'_, NativeConnectionLinkInbox>,
) -> Option<NativeConnectionLinkDelivery> {
    #[cfg(target_os = "android")]
    {
        crate::android_lifecycle::with_presentation_owner(None, |generation| {
            inbox.peek().map(|mut delivery| {
                delivery.activity_generation = Some(generation.to_string());
                delivery
            })
        })
        .flatten()
    }
    #[cfg(not(target_os = "android"))]
    inbox.peek()
}

#[tauri::command]
pub(crate) fn ack_native_connection_link(
    inbox: State<'_, NativeConnectionLinkInbox>,
    revision: String,
    activity_generation: Option<String>,
) -> bool {
    #[cfg(target_os = "android")]
    let accepted = activity_generation.as_deref().is_some_and(|generation| {
        crate::android_lifecycle::with_presentation_owner(Some(generation), |_| {
            inbox.acknowledge(&revision)
        })
        .unwrap_or(false)
    });
    #[cfg(not(target_os = "android"))]
    let accepted = activity_generation.is_none() && inbox.acknowledge(&revision);
    tracing::debug!(
        accepted,
        reason = "native_connection_link_acknowledged",
        "processed a native connection-link presentation acknowledgment"
    );
    accepted
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP_A: &str = "ratspeak://tcp?v=1&host=a.example&port=4242";
    const TCP_B: &str = "ratspeak://tcp?v=1&host=b.example&ifac_name=net&ifac_pass=secret";
    const RADIO: &str = "ratspeak://radio?v=1&region=europe&preset=medium_fast";

    #[test]
    fn android_runtime_filter_adds_connection_hosts_next_to_channel() {
        let mut config: tauri::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        crate::channel_deep_link::configure_android_runtime(&mut config);
        extend_android_runtime(&mut config);
        let mobile = config.plugins.0["deep-link"]["mobile"].as_array().unwrap();
        let hosts: Vec<_> = mobile.iter().map(|e| e["host"].as_str().unwrap()).collect();
        assert_eq!(hosts, ["channel", "tcp", "radio"]);
        assert!(
            mobile
                .iter()
                .all(|e| e["scheme"] == serde_json::json!(["ratspeak"]))
        );
    }

    #[test]
    fn inbox_keeps_only_the_latest_valid_link() {
        let inbox = NativeConnectionLinkInbox::default();
        assert!(inbox.accept_payloads([TCP_A, RADIO]));
        let delivery = inbox.peek().expect("latest link");
        assert!(matches!(delivery.target, ConnectionLinkTarget::Radio(_)));
        assert!(inbox.acknowledge(&delivery.revision));
        assert!(inbox.peek().is_none());
    }

    #[test]
    fn invalid_or_channel_urls_never_replace_a_pending_link() {
        let inbox = NativeConnectionLinkInbox::default();
        assert!(inbox.accept_payloads([TCP_A]));
        assert!(!inbox.accept_payloads([
            "ratspeak://channel?v=1&hub=00112233445566778899aabbccddeeff",
            "ratspeak://tcp?v=1",
            "https://tcp.invalid/",
        ]));
        let delivery = inbox.peek().expect("original link");
        assert!(
            matches!(delivery.target, ConnectionLinkTarget::Tcp(ref t) if t.host == "a.example")
        );
    }

    #[test]
    fn repeated_pending_link_coalesces_but_reopens_after_ack() {
        let inbox = NativeConnectionLinkInbox::default();
        assert!(inbox.accept_payloads([TCP_B]));
        let first = inbox.peek().unwrap();
        assert!(!inbox.accept_payloads([TCP_B]));
        assert_eq!(inbox.peek(), Some(first.clone()));
        assert!(inbox.acknowledge(&first.revision));
        assert!(inbox.accept_payloads([TCP_B]));
        let next = inbox.peek().unwrap();
        assert_ne!(first.revision, next.revision);
        assert!(!inbox.acknowledge(&first.revision));
    }

    #[test]
    fn stale_or_malformed_ack_never_removes_a_newer_link() {
        let inbox = NativeConnectionLinkInbox::default();
        assert!(inbox.accept_payloads([TCP_A]));
        let stale = inbox.peek().unwrap();
        assert!(inbox.accept_payloads([RADIO]));
        let current = inbox.peek().unwrap();
        for revision in [stale.revision.as_str(), "", "0", "02", "+2", "2 "] {
            assert!(!inbox.acknowledge(revision));
            assert_eq!(inbox.peek(), Some(current.clone()));
        }
        assert!(inbox.acknowledge(&current.revision));
    }

    #[test]
    fn exhausted_revisions_never_wrap_or_replace_the_current_link() {
        let inbox = NativeConnectionLinkInbox::default();
        inbox.state().last_revision = u64::MAX - 1;
        assert!(inbox.accept_payloads([TCP_A]));
        let last = inbox.peek().unwrap();
        assert_eq!(last.revision, u64::MAX.to_string());
        assert!(!inbox.accept_payloads([RADIO]));
        assert_eq!(inbox.peek(), Some(last));
    }
}
