//! System notifications through GPUI's `App::show_system_notification`.

use std::cell::Cell;
use std::sync::Mutex;

use gpui::{App, SystemNotification, SystemNotificationAction};
use napi::threadsafe_function::ThreadsafeFunctionCallMode;
use objc2_foundation::NSBundle;

use crate::renderer::{NotificationResponse, NotificationResponseFn, NotificationSpec};

static RESPONSE_JS: Mutex<Option<NotificationResponseFn>> = Mutex::new(None);

thread_local! {
    static SUBSCRIBED: Cell<bool> = const { Cell::new(false) };
}

/// `UNUserNotificationCenter` only works inside an app bundle. GPUI turns
/// notifications off without one, which is how `bun app.tsx` runs.
pub(crate) fn available() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

pub(crate) fn show(spec: NotificationSpec, cx: &mut App) {
    let actions = spec.actions.unwrap_or_default().into_iter();
    cx.show_system_notification(SystemNotification {
        tag: spec.tag.into(),
        title: spec.title.into(),
        body: spec.body.into(),
        actions: actions
            .map(|action| SystemNotificationAction {
                id: action.id.into(),
                label: action.label.into(),
            })
            .collect(),
    });
}

pub(crate) fn dismiss(tag: &str, cx: &mut App) {
    cx.dismiss_system_notification(tag);
}

/// The first subscription makes GPUI open the notification center, so this
/// waits for JS to ask rather than subscribing at startup.
pub(crate) fn set_response(respond: Option<NotificationResponseFn>, cx: &mut App) {
    let subscribing = respond.is_some();
    *RESPONSE_JS.lock().unwrap() = respond;
    if !subscribing || SUBSCRIBED.with(|subscribed| subscribed.replace(true)) {
        return;
    }
    cx.on_system_notification_response(|response, _cx| {
        if let Some(callback) = RESPONSE_JS.lock().unwrap().as_ref() {
            let response = NotificationResponse {
                tag: response.tag.to_string(),
                action_id: response.action_id.map(|id| id.to_string()),
            };
            callback.call(response, ThreadsafeFunctionCallMode::NonBlocking);
        }
    });
}
