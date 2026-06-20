//! 订阅域：公开订阅计划 + C 端会员中心概览。
//!
//! 对齐 `subscription/index.ts`。业务方法经 declaration-merging 模式落在 [`client`] 的
//! `impl Client` 块。本 mod 同时承载 **P7 商品化 7 域共用的 commerce 请求 helper**
//! （`commerce_get` / `commerce_post` / `commerce_get_list` / ...），对齐 TS 各域统一的
//! `doJSON<APIResponse<T>>` 后 `.data` / `.data ?? []` / `.data ?? {}` 解包语义。

pub mod client;
pub mod types;

pub use types::{
    Membership, RolloverPolicy, SubscriptionAudience, SubscriptionPlan, SubscriptionPrecheckResult,
    SubscriptionTier, UserSubscription,
};

use crate::core::client::Client;
use crate::shared::{ApiResponse, Error, Result};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;

impl Client {
    /// 商品化域内部：GET `{api}{path}` → `ApiResponse<T>` 解包 → `data`（空体强 Err）。
    /// 对应 TS `doJSON<APIResponse<T>>('GET',...)` 后取 `.data`（无默认值场景）。
    pub(crate) async fn commerce_get<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        let (env, _) = self
            .do_json_full::<ApiResponse<T>>(reqwest::Method::GET, path, None, signal)
            .await?;
        unwrap_commerce(path, env)
    }

    /// 商品化域内部：GET → `ApiResponse<Vec<T>>` → `data ?? []`（空 data / 空体均回退空 Vec）。
    /// 对应 TS `Array.isArray(resp.data) ? resp.data : []` / `resp.data ?? []`。
    pub(crate) async fn commerce_get_list<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<T>> {
        // 空 data 字段在 ApiResponse<Vec<T>> 下会反序列化失败（data 必填）；这里用宽容包装：
        // 先取 ApiResponse<Option<Vec<T>>> 把 null/缺省吸收为 None → [].
        let (env, _) = self
            .do_json_full::<ApiResponse<Option<Vec<T>>>>(reqwest::Method::GET, path, None, signal)
            .await?;
        match env {
            Some(env) => {
                if let Some(err) = env.business_error() {
                    return Err(err);
                }
                Ok(env.data.unwrap_or_default())
            }
            // 空响应体 → 空数组（TS `?? []`）。
            None => Ok(Vec::new()),
        }
    }

    /// 商品化域内部：POST `{api}{path}`（可选 JSON body）→ `ApiResponse<T>` → `data`（空体强 Err）。
    pub(crate) async fn commerce_post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<T> {
        let (env, _) = self
            .do_json_full::<ApiResponse<T>>(reqwest::Method::POST, path, body, signal)
            .await?;
        unwrap_commerce(path, env)
    }

    /// 商品化域内部：POST → `ApiResponse<Option<T>>` → `Option<data>`（空 data / 空体 → None）。
    /// 用于「空 data 回退默认值」的域（casehall/enterprise getter 端点）。
    pub(crate) async fn commerce_post_opt<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<Option<T>> {
        let (env, _) = self
            .do_json_full::<ApiResponse<Option<T>>>(reqwest::Method::POST, path, body, signal)
            .await?;
        match env {
            Some(env) => {
                if let Some(err) = env.business_error() {
                    return Err(err);
                }
                Ok(env.data)
            }
            None => Ok(None),
        }
    }

    /// 商品化域内部：GET → `ApiResponse<Option<T>>` → `Option<data>`（空 data / 空体 → None）。
    pub(crate) async fn commerce_get_opt<T: DeserializeOwned>(
        &self,
        path: &str,
        signal: Option<CancellationToken>,
    ) -> Result<Option<T>> {
        let (env, _) = self
            .do_json_full::<ApiResponse<Option<T>>>(reqwest::Method::GET, path, None, signal)
            .await?;
        match env {
            Some(env) => {
                if let Some(err) = env.business_error() {
                    return Err(err);
                }
                Ok(env.data)
            }
            None => Ok(None),
        }
    }

    /// 商品化域内部：POST（无返回体语义，仅校验业务码）。对应 TS `await doJSON<...>` 丢弃返回。
    /// 空体跳业务码（§4.4）；非空体校验 `code`。
    pub(crate) async fn commerce_post_discard(
        &self,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let (env, _) = self
            .do_json_full::<ApiResponse<serde_json::Value>>(
                reqwest::Method::POST,
                path,
                body,
                signal,
            )
            .await?;
        if let Some(env) = env {
            if let Some(err) = env.business_error() {
                return Err(err);
            }
        }
        Ok(())
    }

    /// 商品化域内部：POST → `ApiResponse<bool>` → `data ?? false`（空 data / 空体 → false）。
    /// 对应 TS `resp.data ?? false`（upload-proof / revoke）。
    pub(crate) async fn commerce_post_bool(
        &self,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
    ) -> Result<bool> {
        let (env, _) = self
            .do_json_full::<ApiResponse<Option<bool>>>(reqwest::Method::POST, path, body, signal)
            .await?;
        match env {
            Some(env) => {
                if let Some(err) = env.business_error() {
                    return Err(err);
                }
                Ok(env.data.unwrap_or(false))
            }
            None => Ok(false),
        }
    }
}

/// `ApiResponse<T>` → `data`（`code != 0` 抛 BusinessError；空体抛强 Err）。
fn unwrap_commerce<T>(path: &str, env: Option<ApiResponse<T>>) -> Result<T> {
    let env = env.ok_or_else(|| Error::other(format!("{path}: empty response body")))?;
    if let Some(err) = env.business_error() {
        return Err(err);
    }
    Ok(env.data)
}
