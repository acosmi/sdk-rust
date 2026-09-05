//! 通知 / WebSocket 推送域。
//!
//! 对齐 `notifications/index.ts`。类型在 [`types`]；REST 业务方法在 [`notifications`]，
//! WebSocket 长连接在 the optional ws module（均为 declaration-merging 模式的 `impl Client` 块，无 side-effect import）。

#[allow(clippy::module_inception)]
pub mod notifications;
pub mod types;
#[cfg(feature = "notifications-ws")]
pub mod ws;

#[cfg(feature = "notifications-ws")]
pub(crate) use ws::WsHandle;

pub use types::{
    parse_notification_event, DeviceRegistration, Notification, NotificationList,
    NotificationPreference, NotificationUnreadCount, WSEvent,
};
#[cfg(feature = "notifications-ws")]
pub use ws::WSConfig;
