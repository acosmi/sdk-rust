//! 通知 / WebSocket 推送域类型。
//!
//! 端口自 `notifications/types.ts`（其端口自 `acosmi-sdk-go/types.go` v0.19.0）。
//!
//! 命名约定：字段名 = Go json tag 字面量（wire format）。

use serde::{Deserialize, Serialize};

// =============================================================================
// Notifications
// =============================================================================

/// 单条通知。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub content: String,
    /// system | billing | security | task | commission | entitlement。
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(rename = "isRead")]
    pub is_read: bool,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// 分页通知列表。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationList {
    pub list: Vec<Notification>,
    #[serde(rename = "unreadCount")]
    pub unread_count: i64,
    pub total: i64,
    pub page: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
}

/// 未读通知计数。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationUnreadCount {
    #[serde(rename = "unreadCount")]
    pub unread_count: i64,
}

/// 通知偏好（按类型+渠道）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationPreference {
    #[serde(rename = "typeCode")]
    pub type_code: String,
    #[serde(rename = "channelInApp")]
    pub channel_in_app: bool,
    #[serde(rename = "channelEmail")]
    pub channel_email: bool,
    #[serde(rename = "channelSms")]
    pub channel_sms: bool,
    #[serde(rename = "channelPush")]
    pub channel_push: bool,
}

/// 推送设备注册。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceRegistration {
    /// android | ios | harmony。
    pub platform: String,
    pub token: String,
    #[serde(rename = "appVersion")]
    pub app_version: String,
}

// =============================================================================
// WebSocket 类型（forward-declared 这里，实现在 ws.rs）
// =============================================================================

/// 服务端推送事件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WSEvent {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// `json.RawMessage` in Go。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(rename = "connId", default, skip_serializing_if = "Option::is_none")]
    pub conn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 从 [`WSEvent`] 中解析通知。
///
/// 返回 `None` 表示该事件不是系统通知（对应 TS `parseNotificationEvent`）。
pub fn parse_notification_event(ev: &WSEvent) -> Option<Notification> {
    if ev.r#type != "event" || ev.topic.as_deref() != Some("system") {
        return None;
    }
    let data = ev.data.as_ref()?;
    // ev.data 在 wire 上可能是 raw JSON 串或已反序列化对象，两种形态都接住。
    let n: Notification = if let Some(s) = data.as_str() {
        serde_json::from_str(s).ok()?
    } else {
        serde_json::from_value(data.clone()).ok()?
    };
    if n.id.is_empty() {
        return None;
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_notification_event_object() {
        let ev = WSEvent {
            r#type: "event".to_string(),
            topic: Some("system".to_string()),
            data: Some(serde_json::json!({
                "id": "n1", "title": "T", "content": "C", "type": "system",
                "isRead": false, "createdAt": "2026-06-19T00:00:00Z"
            })),
            ..Default::default()
        };
        let n = parse_notification_event(&ev).unwrap();
        assert_eq!(n.id, "n1");
        assert_eq!(n.r#type, "system");
    }

    #[test]
    fn parse_notification_event_string_data() {
        let ev = WSEvent {
            r#type: "event".to_string(),
            topic: Some("system".to_string()),
            data: Some(serde_json::Value::String(
                r#"{"id":"n2","title":"T","content":"C","type":"billing","isRead":true,"createdAt":"x"}"#
                    .to_string(),
            )),
            ..Default::default()
        };
        let n = parse_notification_event(&ev).unwrap();
        assert_eq!(n.id, "n2");
        assert!(n.is_read);
    }

    #[test]
    fn parse_notification_event_non_system_returns_none() {
        let ev = WSEvent {
            r#type: "event".to_string(),
            topic: Some("billing".to_string()),
            data: Some(serde_json::json!({"id":"x"})),
            ..Default::default()
        };
        assert!(parse_notification_event(&ev).is_none());
        // type != event
        let ev2 = WSEvent {
            r#type: "welcome".to_string(),
            topic: Some("system".to_string()),
            ..Default::default()
        };
        assert!(parse_notification_event(&ev2).is_none());
    }
}
