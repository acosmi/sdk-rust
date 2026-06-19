//! 通知业务方法。端口自 `notifications/notifications.ts`
//! （declaration-merging → 此处 `impl Client` 块）。

use super::types::{
    DeviceRegistration, NotificationList, NotificationPreference, NotificationUnreadCount,
};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::core::http::DEFAULT_JSON_TIMEOUT_MS;
use crate::shared::{ApiResponse, Error, Result};
use tokio_util::sync::CancellationToken;

impl Client {
    /// 分页查询通知列表。对应 TS `listNotifications`。
    pub async fn list_notifications(
        &self,
        page: i64,
        page_size: i64,
        type_filter: &str,
        signal: Option<CancellationToken>,
    ) -> Result<NotificationList> {
        let mut path = format!("/notifications?page={page}&pageSize={page_size}");
        if !type_filter.is_empty() {
            path.push_str(&format!("&type={}", urlencoding(type_filter)));
        }
        self.billing_get(&path, signal).await
    }

    /// 获取未读通知数量。对应 TS `getUnreadCount`。
    pub async fn get_unread_count(&self, signal: Option<CancellationToken>) -> Result<i64> {
        let resp: NotificationUnreadCount = self
            .billing_get("/notifications/unread-count", signal)
            .await?;
        Ok(resp.unread_count)
    }

    /// 标记单条通知已读。对应 TS `markNotificationRead`。
    pub async fn mark_notification_read(
        &self,
        id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.notify_void(
            reqwest::Method::PUT,
            &format!("/notifications/{}/read", urlencoding(id)),
            None,
            signal,
        )
        .await
    }

    /// 标记全部通知已读。对应 TS `markAllNotificationsRead`。
    pub async fn mark_all_notifications_read(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.notify_void(
            reqwest::Method::PUT,
            "/notifications/read-all",
            None,
            signal,
        )
        .await
    }

    /// 删除通知。对应 TS `deleteNotification`。
    pub async fn delete_notification(
        &self,
        id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.notify_void(
            reqwest::Method::DELETE,
            &format!("/notifications/{}", urlencoding(id)),
            None,
            signal,
        )
        .await
    }

    /// 注册推送设备 token。对应 TS `registerDevice`。
    ///
    /// **@experimental** 网关尚未提供 `/devices/*` 与 `/notification-preferences/*` 端点，调用将 404。
    /// 后端实现落地前请勿在生产使用。
    pub async fn register_device(
        &self,
        reg: &DeviceRegistration,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let body = serde_json::to_string(reg)
            .map_err(|e| Error::other(format!("serialize device registration: {e}")))?;
        self.notify_void(
            reqwest::Method::POST,
            "/devices/register",
            Some(&body),
            signal,
        )
        .await
    }

    /// 注销推送设备 token。对应 TS `unregisterDevice`。
    ///
    /// **@experimental** 见 [`Self::register_device`]。
    pub async fn unregister_device(
        &self,
        token: &str,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        self.notify_void(
            reqwest::Method::DELETE,
            &format!("/devices/{}", urlencoding(token)),
            None,
            signal,
        )
        .await
    }

    /// 获取通知偏好设置。对应 TS `listNotificationPreferences`。
    ///
    /// **@experimental** 见 [`Self::register_device`]。
    pub async fn list_notification_preferences(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<NotificationPreference>> {
        self.billing_get("/notification-preferences", signal).await
    }

    /// 更新通知偏好。对应 TS `updateNotificationPreference`。
    ///
    /// **@experimental** 见 [`Self::register_device`]。
    pub async fn update_notification_preference(
        &self,
        type_code: &str,
        pref: &NotificationPreference,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let body = serde_json::to_string(pref)
            .map_err(|e| Error::other(format!("serialize preference: {e}")))?;
        self.notify_void(
            reqwest::Method::PUT,
            &format!("/notification-preferences/{}", urlencoding(type_code)),
            Some(&body),
            signal,
        )
        .await
    }

    /// 通知域内部：发起返回 `ApiResponse<unknown>` 的写端点（PUT/DELETE/POST），仅做业务码检查。
    /// 对应 TS `doJSON<APIResponse<unknown>>(...)`（忽略 data）。
    async fn notify_void(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let (bytes, _) = self
            .do_json_full_raw(method, path, body, signal, DEFAULT_JSON_TIMEOUT_MS)
            .await?;
        // 空体成功 → 跳业务码检查（方案 §4.4）。
        if bytes.is_empty() {
            return Ok(());
        }
        let env: ApiResponse<serde_json::Value> = serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("{path}: decode: {e}")))?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        Ok(())
    }
}
