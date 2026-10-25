//! Keys and phrases the user copies stay out of clipboard history where the
//! platform allows, and leave the clipboard after a minute, and on desktop
//! when the app quits, unless something else was copied since.

use zeroize::Zeroizing;

/// How long a copied secret stays on the clipboard.
const CLEAR_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

#[cfg(not(target_os = "ios"))]
use clear::Clear;

/// The clear is a promise in clock time: its deadline runs on the clock that
/// counts suspend, and its wait re-checks it at least every `RECHECK`, never
/// one sleep that would count awake time only. iOS expires the copy itself.
#[cfg(any(not(target_os = "ios"), test))]
mod clear {
    use std::time::Duration;

    use ratspeak_tauri::state::ContinuousInstant;

    const RECHECK: Duration = Duration::from_secs(5);

    /// When a copied secret leaves the clipboard.
    #[derive(Clone, Copy)]
    pub(super) struct Clear {
        pub(super) copied_at: ContinuousInstant,
    }

    impl Clear {
        /// How long to wait before checking again, or `None` once it is due.
        pub(super) fn wait(self, now: ContinuousInstant) -> Option<Duration> {
            super::CLEAR_AFTER
                .checked_sub(now.saturating_duration_since(self.copied_at))
                .filter(|left| !left.is_zero())
                .map(|left| left.min(RECHECK))
        }
    }
}

#[tauri::command]
pub async fn copy_secret(app: tauri::AppHandle, text: String) -> Result<(), String> {
    let text = Zeroizing::new(text);
    #[cfg(target_os = "ios")]
    {
        let (reply, copied) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = reply.send(ios::copy(&text));
        })
        .map_err(|_| UNAVAILABLE.to_string())?;
        tauri::async_runtime::spawn_blocking(move || copied.recv())
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(Result::ok)
            .ok_or_else(|| UNAVAILABLE.to_string())
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = app;
        let id = platform::copy(&text)?;
        let clear = Clear {
            copied_at: ratspeak_tauri::state::ContinuousInstant::now(),
        };
        tauri::async_runtime::spawn(async move {
            while let Some(wait) = clear.wait(ratspeak_tauri::state::ContinuousInstant::now()) {
                tokio::time::sleep(wait).await;
            }
            platform::forget(Some(id));
        });
        Ok(())
    }
}

const UNAVAILABLE: &str = "The clipboard is unavailable";

/// Clears the newest secret now, if it is still on the clipboard.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn forget_now() {
    platform::forget(None);
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod platform {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, PoisonError};

    use arboard::Clipboard;
    use zeroize::Zeroizing;

    struct Copied {
        id: u64,
        // Kept open: on X11 the copy lives only while its owner does.
        clipboard: Clipboard,
        #[cfg(target_os = "macos")]
        change_count: isize,
        #[cfg(not(target_os = "macos"))]
        text: Zeroizing<String>,
    }

    /// The newest secret copied, until it is cleared.
    static COPIED: Mutex<Option<Copied>> = Mutex::new(None);
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    pub(super) fn copy(text: &Zeroizing<String>) -> Result<u64, String> {
        let unavailable = |_| super::UNAVAILABLE.to_string();
        let mut copied = COPIED.lock().unwrap_or_else(PoisonError::into_inner);
        let mut clipboard = Clipboard::new().map_err(unavailable)?;
        let set = clipboard.set();
        #[cfg(target_os = "macos")]
        let set = arboard::SetExtApple::exclude_from_history(set);
        #[cfg(windows)]
        let set = arboard::SetExtWindows::exclude_from_monitoring(set);
        #[cfg(all(unix, not(target_os = "macos")))]
        let set = arboard::SetExtLinux::exclude_from_history(set);
        set.text(text.as_str()).map_err(unavailable)?;

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        *copied = Some(Copied {
            id,
            clipboard,
            #[cfg(target_os = "macos")]
            change_count: change_count(),
            #[cfg(not(target_os = "macos"))]
            text: text.clone(),
        });
        Ok(id)
    }

    /// Clears secret `id`, or with `None` the newest, if it is still the
    /// newest and still on the clipboard.
    pub(super) fn forget(id: Option<u64>) {
        let mut copied = COPIED.lock().unwrap_or_else(PoisonError::into_inner);
        if id.is_some_and(|id| copied.as_ref().is_none_or(|copied| copied.id != id)) {
            return;
        }
        let Some(mut copied) = copied.take() else {
            return;
        };
        if still_on_clipboard(&mut copied) {
            let _ = copied.clipboard.clear();
        }
    }

    // Reading the pasteboard can prompt about another app's content; its
    // change count never does.
    #[cfg(target_os = "macos")]
    fn still_on_clipboard(copied: &mut Copied) -> bool {
        change_count() == copied.change_count
    }

    #[cfg(not(target_os = "macos"))]
    fn still_on_clipboard(copied: &mut Copied) -> bool {
        copied
            .clipboard
            .get()
            .text()
            .is_ok_and(|text| Zeroizing::new(text) == copied.text)
    }

    #[cfg(target_os = "macos")]
    fn change_count() -> isize {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject};
        let Some(class) = AnyClass::get(c"NSPasteboard") else {
            return -1;
        };
        // SAFETY: generalPasteboard and changeCount take no arguments.
        unsafe {
            let pasteboard: *mut AnyObject = msg_send![class, generalPasteboard];
            if pasteboard.is_null() {
                return -1;
            }
            msg_send![pasteboard, changeCount]
        }
    }
}

#[cfg(target_os = "android")]
mod platform {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, PoisonError};

    use zeroize::Zeroizing;

    /// The newest secret copied, until it is cleared.
    static COPIED: Mutex<Option<u64>> = Mutex::new(None);
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    pub(super) fn copy(text: &Zeroizing<String>) -> Result<u64, String> {
        let mut copied = COPIED.lock().unwrap_or_else(PoisonError::into_inner);
        if !crate::mobile_native::copy_secret(text) {
            return Err(super::UNAVAILABLE.into());
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        *copied = Some(id);
        Ok(id)
    }

    /// Clears secret `id` if it is still the newest copied here.
    pub(super) fn forget(id: Option<u64>) {
        let mut copied = COPIED.lock().unwrap_or_else(PoisonError::into_inner);
        if id.is_some_and(|id| *copied != Some(id)) {
            return;
        }
        if copied.take().is_some() {
            crate::mobile_native::clear_secret();
        }
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use std::ffi::{c_char, CStr, CString};

    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use zeroize::Zeroizing;

    #[link(name = "UIKit", kind = "framework")]
    unsafe extern "C" {
        static UIPasteboardOptionLocalOnly: *mut AnyObject;
        static UIPasteboardOptionExpirationDate: *mut AnyObject;
    }

    /// On this device only, never on Universal Clipboard, and gone by itself
    /// after `CLEAR_AFTER`. Call on the main thread.
    pub(super) fn copy(text: &str) -> Result<(), ()> {
        let text = Zeroizing::new(CString::new(text).map_err(drop)?.into_bytes_with_nul());
        let class = |name: &CStr| AnyClass::get(name).ok_or(());
        let (string, dictionary, array, date, number, pasteboard) = (
            class(c"NSString")?,
            class(c"NSDictionary")?,
            class(c"NSArray")?,
            class(c"NSDate")?,
            class(c"NSNumber")?,
            class(c"UIPasteboard")?,
        );
        // SAFETY: Foundation and UIKit class methods with valid arguments; the
        // option keys are UIKit's exported constants.
        unsafe {
            let value: *mut AnyObject =
                msg_send![string, stringWithUTF8String: text.as_ptr().cast::<c_char>()];
            let kind: *mut AnyObject =
                msg_send![string, stringWithUTF8String: c"public.utf8-plain-text".as_ptr()];
            if value.is_null() || kind.is_null() {
                return Err(());
            }
            let item: *mut AnyObject =
                msg_send![dictionary, dictionaryWithObject: value, forKey: kind];
            let items: *mut AnyObject = msg_send![array, arrayWithObject: item];
            let expiry: *mut AnyObject =
                msg_send![date, dateWithTimeIntervalSinceNow: super::CLEAR_AFTER.as_secs_f64()];
            let local: *mut AnyObject = msg_send![number, numberWithBool: Bool::YES];
            let keys = [
                UIPasteboardOptionLocalOnly,
                UIPasteboardOptionExpirationDate,
            ];
            let values = [local, expiry];
            let options: *mut AnyObject = msg_send![
                dictionary,
                dictionaryWithObjects: values.as_ptr(),
                forKeys: keys.as_ptr(),
                count: values.len()
            ];
            let general: *mut AnyObject = msg_send![pasteboard, generalPasteboard];
            if item.is_null() || items.is_null() || options.is_null() || general.is_null() {
                return Err(());
            }
            let _: () = msg_send![general, setItems: items, options: options];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratspeak_tauri::state::ContinuousInstant;

    use super::clear::Clear;

    // The minute counts time the system slept, and the wait re-checks it at
    // least every five seconds, so a clear that fell due during sleep happens
    // within seconds of waking.
    #[test]
    fn a_copied_secret_leaves_a_minute_after_the_copy_in_clock_time() {
        let copied_at = ContinuousInstant::now();
        let clear = Clear { copied_at };
        let at = |secs: u64| copied_at + Duration::from_secs(secs);
        assert_eq!(clear.wait(copied_at), Some(Duration::from_secs(5)));
        assert_eq!(clear.wait(at(57)), Some(Duration::from_secs(3)));
        assert_eq!(clear.wait(at(59)), Some(Duration::from_secs(1)));
        assert_eq!(clear.wait(at(60)), None);
        // A lid closed for eight hours.
        assert_eq!(clear.wait(at(8 * 3600)), None);
        // A clock that reads earlier than the copy waits the whole minute.
        assert_eq!(
            clear.wait(copied_at.checked_sub(Duration::from_secs(1)).unwrap()),
            Some(Duration::from_secs(5))
        );
    }
}
