#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeNotificationKind {
    Message,
    Channel,
    Game,
    Call,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeNotification {
    pub kind: NativeNotificationKind,
    pub title: String,
    pub body: String,
    pub thread_id: Option<String>,
    pub notification_id: Option<i32>,
}

impl NativeNotification {
    pub fn message(
        title: impl Into<String>,
        body: impl Into<String>,
        thread_id: impl Into<String>,
        notification_id: i32,
    ) -> Self {
        Self {
            kind: NativeNotificationKind::Message,
            title: title.into(),
            body: body.into(),
            thread_id: Some(thread_id.into()),
            notification_id: Some(notification_id),
        }
    }

    pub fn game(
        title: impl Into<String>,
        body: impl Into<String>,
        thread_id: impl Into<String>,
        notification_id: i32,
    ) -> Self {
        Self {
            kind: NativeNotificationKind::Game,
            title: title.into(),
            body: body.into(),
            thread_id: Some(thread_id.into()),
            notification_id: Some(notification_id),
        }
    }

    pub fn channel(
        title: impl Into<String>,
        body: impl Into<String>,
        thread_id: impl Into<String>,
        notification_id: i32,
    ) -> Self {
        Self {
            kind: NativeNotificationKind::Channel,
            title: title.into(),
            body: body.into(),
            thread_id: Some(thread_id.into()),
            notification_id: Some(notification_id),
        }
    }

    pub fn call(
        title: impl Into<String>,
        body: impl Into<String>,
        thread_id: impl Into<String>,
        notification_id: i32,
    ) -> Self {
        Self {
            kind: NativeNotificationKind::Call,
            title: title.into(),
            body: body.into(),
            thread_id: Some(thread_id.into()),
            notification_id: Some(notification_id),
        }
    }
}

pub trait NativeNotifier: Send + Sync {
    fn notify(&self, notification: NativeNotification);
    /// Removes every alert this app still shows, where the platform allows.
    fn withdraw_all(&self);
}

pub struct NoopNotifier;

impl NativeNotifier for NoopNotifier {
    fn notify(&self, _notification: NativeNotification) {}
    fn withdraw_all(&self) {}
}

/// How much of a message an alert shows; stored as `notification_preview`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotificationPreview {
    #[default]
    NameAndMessage,
    NameOnly,
    Neither,
}

impl NotificationPreview {
    /// As stored, and as the IPC surface names it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NameAndMessage => "name_and_message",
            Self::NameOnly => "name_only",
            Self::Neither => "neither",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "name_and_message" => Some(Self::NameAndMessage),
            "name_only" => Some(Self::NameOnly),
            "neither" => Some(Self::Neither),
            _ => None,
        }
    }
}

/// The alert as `preview` allows. A call always names the caller.
pub fn with_preview(
    mut notification: NativeNotification,
    preview: NotificationPreview,
) -> NativeNotification {
    use NativeNotificationKind::{Call, Channel, Game, Message};
    match (preview, &notification.kind) {
        (NotificationPreview::NameAndMessage, _) | (_, Call) => {}
        (NotificationPreview::NameOnly, Message | Channel) => {
            notification.body = "New message".into();
        }
        (NotificationPreview::NameOnly, Game) => {}
        (NotificationPreview::Neither, kind) => {
            notification.body = if *kind == Game {
                "New game activity"
            } else {
                "New message"
            }
            .into();
            notification.title = "Ratspeak".into();
        }
    }
    notification
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(kind: NativeNotificationKind, preview: NotificationPreview) -> (String, String) {
        let alert = NativeNotification {
            kind,
            title: "Message from Alice".into(),
            body: "meet at noon".into(),
            thread_id: Some("lxmf:ab".into()),
            notification_id: Some(7),
        };
        let shown = with_preview(alert, preview);
        assert_eq!(
            shown.thread_id.as_deref(),
            Some("lxmf:ab"),
            "the route stays"
        );
        assert_eq!(shown.notification_id, Some(7));
        (shown.title, shown.body)
    }

    #[test]
    fn alerts_show_what_the_preview_allows() {
        use NativeNotificationKind::{Call, Channel, Game, Message};
        use NotificationPreview::{NameAndMessage, NameOnly, Neither};
        let full = ("Message from Alice".to_string(), "meet at noon".to_string());
        for kind in [Message, Channel, Game, Call] {
            assert_eq!(shown(kind, NameAndMessage), full);
        }
        for kind in [Message, Channel] {
            assert_eq!(
                shown(kind.clone(), NameOnly),
                ("Message from Alice".into(), "New message".into())
            );
            assert_eq!(
                shown(kind, Neither),
                ("Ratspeak".into(), "New message".into())
            );
        }
        assert_eq!(
            shown(Game, NameOnly),
            full,
            "a game alert quotes no message"
        );
        assert_eq!(
            shown(Game, Neither),
            ("Ratspeak".into(), "New game activity".into())
        );
        assert_eq!(shown(Call, Neither), full, "a call names its caller");
    }

    #[test]
    fn previews_read_back_as_stored() {
        for preview in [
            NotificationPreview::NameAndMessage,
            NotificationPreview::NameOnly,
            NotificationPreview::Neither,
        ] {
            assert_eq!(NotificationPreview::parse(preview.as_str()), Some(preview));
        }
        assert_eq!(NotificationPreview::parse("everything"), None);
        assert_eq!(
            NotificationPreview::default(),
            NotificationPreview::NameAndMessage
        );
    }
}
