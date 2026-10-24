//! Native `ratspeak://connection` lifecycle integration.
//!
//! Same contract as `channel_deep_link`: the frontend is not granted the
//! plugin command API and never subscribes to the plugin's URL event.
//! Platform-delivered URLs cross the canonical Rust parser once; only the
//! typed, secret-free pre-fill values are retained in this bounded
//! process-memory inbox until application JavaScript acknowledges that a
//! visible page presented them. Presenting a target only pre-fills the normal
//! Internet/TCP or Radio sheet; it never adds, edits or connects an interface.

use std::sync::{Mutex, MutexGuard};

use ratspeak_tauri::commands::connection_share::{
    parse_connection_share_target, ConnectionShareTarget,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_deep_link::DeepLinkExt;
use url::Url;

const NATIVE_CONNECTION_LINK_AVAILABLE: &str = "native_connection_link_available";

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
    target: ConnectionShareTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeConnectionLinkDelivery {
    // JavaScript numbers cannot represent every u64 exactly. This token is
    // opaque to the frontend and echoed byte-for-byte in the acknowledgment.
    revision: String,
    activity_generation: Option<String>,
    target: ConnectionShareTarget,
}

impl NativeConnectionLinkInbox {
    fn state(&self) -> MutexGuard<'_, InboxState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Accept every valid target in arrival order and retain only the newest.
    ///
    /// Returning `true` means the observable pending value changed. Repeated
    /// delivery of the same URL while it is still pending is coalesced.
    fn accept_payloads<'a>(&self, payloads: impl IntoIterator<Item = &'a str>) -> bool {
        let mut latest = None;
        let mut rejected = 0usize;
        for payload in payloads {
            match parse_connection_share_target(payload) {
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
        // Never reuse a delivery token, even at exhaustion. Retain any current
        // target instead of letting an old acknowledgment remove a newer one.
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

/// A peek blocked by Android's native lifecycle is not an empty inbox. Wake
/// the visible frontend again after native readiness/resume, without polling
/// and without holding either the lifecycle authority or the inbox lock.
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
/// The inbox coalesces the harmless overlap if both paths report the same URL.
/// The plugin supports several listeners, so this coexists with the channel one.
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

    const TCP_A: &str = "ratspeak://connection?v=1&kind=tcp&host=a.example&port=4242";
    const TCP_B: &str = "ratspeak://connection?v=1&kind=tcp&host=b.example&port=4243&name=B";
    const RADIO: &str =
        "ratspeak://connection?v=1&kind=radio&freq=868000000&bw=125000&sf=8&cr=5&tx=14";

    #[test]
    fn inbox_keeps_only_the_latest_canonical_target() {
        let inbox = NativeConnectionLinkInbox::default();
        assert!(inbox.accept_payloads([TCP_A, RADIO, TCP_B]));
        let delivery = inbox.peek().expect("latest target");
        assert_eq!(delivery.target.host.as_deref(), Some("b.example"));
        assert!(inbox.acknowledge(&delivery.revision));
        assert!(inbox.peek().is_none());
    }

    #[test]
    fn invalid_secret_bearing_or_foreign_urls_never_replace_a_pending_target() {
        let inbox = NativeConnectionLinkInbox::default();
        assert!(inbox.accept_payloads([TCP_A]));
        let with_secret = format!("{TCP_A}&passphrase=secret");
        let channel = "ratspeak://channel?v=1&hub=00112233445566778899aabbccddeeff";
        assert!(!inbox.accept_payloads([
            with_secret.as_str(),
            channel,
            "ratspeak://contact?v=1",
            "https://connection.invalid/"
        ]));
        assert_eq!(
            inbox.peek().expect("original target").target.host.as_deref(),
            Some("a.example")
        );
    }

    #[test]
    fn repeated_pending_target_coalesces_but_reopens_with_a_new_revision_after_ack() {
        let inbox = NativeConnectionLinkInbox::default();
        assert!(inbox.accept_payloads([RADIO]));
        let first = inbox.peek().unwrap();
        assert!(!inbox.accept_payloads([RADIO]));
        assert_eq!(inbox.peek(), Some(first.clone()));
        assert!(inbox.acknowledge(&first.revision));
        assert!(inbox.accept_payloads([RADIO]));
        let next = inbox.peek().unwrap();
        assert_ne!(first.revision, next.revision);
        assert!(!inbox.acknowledge(&first.revision));
        assert_eq!(inbox.peek(), Some(next));
    }

    #[test]
    fn stale_or_malformed_ack_never_removes_a_newer_target() {
        let inbox = NativeConnectionLinkInbox::default();
        assert!(inbox.accept_payloads([TCP_A]));
        let stale = inbox.peek().unwrap();
        assert!(inbox.accept_payloads([TCP_B]));
        let current = inbox.peek().unwrap();
        for revision in [stale.revision.as_str(), "", "0", "02", "+2", "2 ", "18446744073709551616"]
        {
            assert!(!inbox.acknowledge(revision));
            assert_eq!(inbox.peek(), Some(current.clone()));
        }
        assert!(inbox.acknowledge(&current.revision));
    }

    #[test]
    fn revisions_remain_exact_above_javascript_integer_precision() {
        let inbox = NativeConnectionLinkInbox::default();
        inbox.state().last_revision = 9_007_199_254_740_992;
        assert!(inbox.accept_payloads([TCP_A]));
        let delivery = inbox.peek().unwrap();
        assert_eq!(delivery.revision, "9007199254740993");
        assert!(serde_json::to_value(&delivery).unwrap()["revision"].is_string());
    }

    #[test]
    fn exhausted_revisions_never_wrap_or_replace_the_current_target() {
        let inbox = NativeConnectionLinkInbox::default();
        inbox.state().last_revision = u64::MAX - 1;
        assert!(inbox.accept_payloads([TCP_A]));
        let last = inbox.peek().unwrap();
        assert_eq!(last.revision, u64::MAX.to_string());
        assert!(!inbox.accept_payloads([TCP_B]));
        assert_eq!(inbox.peek(), Some(last.clone()));
        assert!(inbox.acknowledge(&last.revision));
        assert!(!inbox.accept_payloads([TCP_B]));
        assert!(inbox.peek().is_none());
    }
}
