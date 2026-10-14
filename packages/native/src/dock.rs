//! The Dock tile. GPUI has no API for its badge or for asking for attention.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSRequestUserAttentionType};
use objc2_foundation::NSString;

pub(crate) fn set_badge(label: Option<&str>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let label = label
        .filter(|label| !label.is_empty())
        .map(NSString::from_str);
    NSApplication::sharedApplication(mtm)
        .dockTile()
        .setBadgeLabel(label.as_deref());
}

/// Bounces the Dock icon once, or until the app comes forward when `critical`.
/// macOS ignores it while the app is already active.
pub(crate) fn request_attention(critical: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let kind = if critical {
        NSRequestUserAttentionType::CriticalRequest
    } else {
        NSRequestUserAttentionType::InformationalRequest
    };
    NSApplication::sharedApplication(mtm).requestUserAttention(kind);
}
