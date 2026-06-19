//! 技能商店 + 生产器业务方法。端口自 `skills/skills.ts`
//! （declaration-merging → 此处 `impl Client` 块）。

use super::types::{
    CertificationStatus, GenerateSkillRequest, GenerateSkillResult, OptimizeSkillRequest,
    OptimizeSkillResult, SkillBrowseListResponse, SkillBrowseResponse, SkillStoreItem,
    SkillStoreQuery, SkillSummary,
};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::core::http::{
    classify_transport, parse_http_error_with_retry_after, read_limited, read_limited_text,
    MAX_DOWNLOAD_SIZE, MAX_ERROR_BODY_SIZE,
};
use crate::shared::{ApiResponse, Error, Result};
use tokio_util::sync::CancellationToken;

/// 技能 ZIP 下载/上传超时（5min，大文件）。
const SKILL_TRANSFER_TIMEOUT_MS: u64 = 5 * 60 * 1000;

/// 技能 ZIP 下载结果。对应 TS `{ data, filename }`。
#[derive(Debug, Clone)]
pub struct SkillDownload {
    pub data: Vec<u8>,
    pub filename: String,
}

impl Client {
    /// 浏览技能商店（公共端点，无需认证）。对应 TS `browseSkillStore`。
    pub async fn browse_skill_store(
        &self,
        query: &SkillStoreQuery,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<SkillStoreItem>> {
        let resp = self
            .browse_skills(
                1,
                50,
                query.category.as_deref().unwrap_or(""),
                query.keyword.as_deref().unwrap_or(""),
                query.tag.as_deref().unwrap_or(""),
                "",
                signal,
            )
            .await?;
        Ok(resp.items)
    }

    /// 浏览公共技能商店（V3 分页接口）。对应 TS `browseSkills`。
    #[allow(clippy::too_many_arguments)]
    pub async fn browse_skills(
        &self,
        page: i64,
        page_size: i64,
        category: &str,
        keyword: &str,
        tag: &str,
        source: &str,
        signal: Option<CancellationToken>,
    ) -> Result<SkillBrowseResponse> {
        let qs = build_skill_query(page, page_size, category, keyword, tag, source, false);
        let env: ApiResponse<SkillBrowseResponse> = self
            .do_public_json_full(&format!("/skill-store?{qs}"), signal)
            .await?;
        unwrap_public(&env)
    }

    /// 轻量浏览公共技能商店（fields=minimal，响应体积缩减 90%+）。对应 TS `browseSkillsList`。
    #[allow(clippy::too_many_arguments)]
    pub async fn browse_skills_list(
        &self,
        page: i64,
        page_size: i64,
        category: &str,
        keyword: &str,
        tag: &str,
        source: &str,
        signal: Option<CancellationToken>,
    ) -> Result<SkillBrowseListResponse> {
        let qs = build_skill_query(page, page_size, category, keyword, tag, source, true);
        let env: ApiResponse<SkillBrowseListResponse> = self
            .do_public_json_full(&format!("/skill-store?{qs}"), signal)
            .await?;
        unwrap_public(&env)
    }

    /// 获取技能商店中某个技能的详情（公共端点）。对应 TS `getSkillDetail`。
    pub async fn get_skill_detail(
        &self,
        skill_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<SkillStoreItem> {
        let env: ApiResponse<SkillStoreItem> = self
            .do_public_json_full(&format!("/skill-store/{}", urlencoding(skill_id)), signal)
            .await?;
        unwrap_public(&env)
    }

    /// 按 key 精确查找公共技能（公共端点）。对应 TS `resolveSkill`。
    pub async fn resolve_skill(
        &self,
        key: &str,
        signal: Option<CancellationToken>,
    ) -> Result<SkillStoreItem> {
        let env: ApiResponse<SkillStoreItem> = self
            .do_public_json_full(
                &format!("/skill-store/resolve/{}", urlencoding(key)),
                signal,
            )
            .await?;
        unwrap_public(&env)
    }

    /// 安装技能到当前用户的租户空间（需 OAuth scope: skill_store）。对应 TS `installSkill`。
    pub async fn install_skill(
        &self,
        skill_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<SkillStoreItem> {
        self.billing_post(
            &format!("/skill-store/{}/install", urlencoding(skill_id)),
            signal,
        )
        .await
    }

    /// 下载技能 ZIP 包（公共端点，双模式）。对应 TS `downloadSkill`。
    ///
    /// 有 token 时自动附带（享受无限流），无 token 时匿名（受限流）。
    /// 50MB 上限走 [`MAX_DOWNLOAD_SIZE`] + [`read_limited`]；限流时抛 [`Error::RateLimit`]。
    pub async fn download_skill(
        &self,
        skill_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<SkillDownload> {
        // 5min 超时（大文件下载）：派生子 token，超时或 parent 取消任一触发即 abort。
        let ctl = self.derive_timeout_token(SKILL_TRANSFER_TIMEOUT_MS, signal);

        let path = format!("/skill-store/{}/download", urlencoding(skill_id));
        let url = self.api_url(&path);

        // 公共端点允许无 token：拿不到 token 时匿名访问。
        let mut rb = self.http().get(&url);
        if let Ok(token) = self.ensure_token(ctl.clone()).await {
            if !token.is_empty() {
                rb = rb.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
            }
        }

        let send = rb.send();
        let resp = match &ctl {
            Some(c) => tokio::select! {
                r = send => r,
                _ = c.cancelled() => return Err(Error::other("download skill: aborted")),
            },
            None => send.await,
        }
        .map_err(|e| Error::Network(classify_transport(&format!("GET {path}"), &url, &e)))?;

        let status = resp.status();
        if status.as_u16() == 429 {
            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body_text = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
            return Err(Error::rate_limit(
                "匿名下载已达限制",
                retry_after,
                body_text,
            ));
        }

        if !status.is_success() {
            let body = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
            let he = parse_http_error_with_retry_after(status.as_u16(), &body, 0);
            return Err(Error::other(format!("download skill: {he}")));
        }

        let headers = resp.headers().clone();
        // 读至 MAX_DOWNLOAD_SIZE + 1：超过即超限报错（对齐 TS）。
        let data = read_limited(resp.bytes_stream(), MAX_DOWNLOAD_SIZE + 1).await?;
        if data.len() > MAX_DOWNLOAD_SIZE {
            return Err(Error::other(format!(
                "download skill: response exceeds {}MB limit",
                MAX_DOWNLOAD_SIZE >> 20
            )));
        }

        let filename = parse_content_disposition_filename(&headers);
        Ok(SkillDownload { data, filename })
    }

    /// 上传技能 ZIP 包。对应 TS `uploadSkill`。
    ///
    /// - `scope`：`"TENANT"`。
    /// - `intent`：`"PERSONAL"`（仅自己用）或 `"PUBLIC_INTENT"`（走认证→公开）。
    pub async fn upload_skill(
        &self,
        zip_data: Vec<u8>,
        scope: &str,
        intent: &str,
        signal: Option<CancellationToken>,
    ) -> Result<SkillStoreItem> {
        self.upload_skill_internal(&zip_data, scope, intent, false, signal)
            .await
    }

    async fn upload_skill_internal(
        &self,
        zip_data: &[u8],
        scope: &str,
        intent: &str,
        retried: bool,
        signal: Option<CancellationToken>,
    ) -> Result<SkillStoreItem> {
        let ctl = self.derive_timeout_token(SKILL_TRANSFER_TIMEOUT_MS, signal.clone());
        let token = self.ensure_token(ctl.clone()).await?;

        // multipart/form-data（对齐 Go mime/multipart）。
        let part = reqwest::multipart::Part::bytes(zip_data.to_vec())
            .file_name("skill.zip")
            .mime_str("application/zip")
            .map_err(|e| Error::other(format!("upload: build part: {e}")))?;
        let form = reqwest::multipart::Form::new()
            .text("scope", scope.to_string())
            .text("intent", intent.to_string())
            .part("file", part);

        let url = self.api_url("/skill-store/upload");
        let send = self
            .http()
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .multipart(form)
            .send();
        let resp = match &ctl {
            Some(c) => tokio::select! {
                r = send => r,
                _ = c.cancelled() => return Err(Error::other("upload: aborted".to_string())),
            },
            None => send.await,
        }
        .map_err(|e| Error::Network(classify_transport("POST /skill-store/upload", &url, &e)))?;

        // 401：单次 force_refresh 重试（防递归）。
        if resp.status().as_u16() == 401 && !retried {
            drop(resp);
            self.force_refresh(ctl.clone()).await.map_err(|e| {
                Error::other(format!("upload: unauthorized and refresh failed: {e}"))
            })?;
            return Box::pin(self.upload_skill_internal(zip_data, scope, intent, true, signal))
                .await;
        }

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
            let he = parse_http_error_with_retry_after(status, &body, 0);
            return Err(Error::other(format!("upload: {he}")));
        }

        // 响应形状：`{ data: { skill: SkillStoreItem } }`。
        let text = resp
            .text()
            .await
            .map_err(|e| Error::other(format!("upload: read body: {e}")))?;
        let env: ApiResponse<UploadData> = serde_json::from_str(&text)
            .map_err(|e| Error::other(format!("upload: decode: {e}")))?;
        if let Some(err) = env.business_error() {
            return Err(err);
        }
        Ok(env.data.skill)
    }

    /// 获取技能统计概览。对应 TS `getSkillSummary`。
    pub async fn get_skill_summary(
        &self,
        signal: Option<CancellationToken>,
    ) -> Result<SkillSummary> {
        self.billing_get("/skills/summary", signal).await
    }

    /// 触发技能认证管线（异步）。对应 TS `certifySkill`。
    pub async fn certify_skill(
        &self,
        skill_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        // POST 端点返回 ApiResponse<unknown>，只需确认成功（业务码检查）。
        let _: serde_json::Value = self
            .billing_post(
                &format!("/skill-store/{}/certify", urlencoding(skill_id)),
                signal,
            )
            .await?;
        Ok(())
    }

    /// 查询技能认证状态。对应 TS `getCertificationStatus`。
    pub async fn get_certification_status(
        &self,
        skill_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<CertificationStatus> {
        self.billing_get(
            &format!("/skill-store/{}/certification", urlencoding(skill_id)),
            signal,
        )
        .await
    }

    /// 根据自然语言描述生成技能定义（基于独立 LLM）。对应 TS `generateSkill`。
    pub async fn generate_skill(
        &self,
        req: &GenerateSkillRequest,
        signal: Option<CancellationToken>,
    ) -> Result<GenerateSkillResult> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("serialize generate request: {e}")))?;
        self.billing_post_body("/skill-generator/generate", Some(&body), signal)
            .await
    }

    /// 优化已有技能定义。对应 TS `optimizeSkill`。
    pub async fn optimize_skill(
        &self,
        req: &OptimizeSkillRequest,
        signal: Option<CancellationToken>,
    ) -> Result<OptimizeSkillResult> {
        let body = serde_json::to_string(req)
            .map_err(|e| Error::other(format!("serialize optimize request: {e}")))?;
        self.billing_post_body("/skill-generator/optimize", Some(&body), signal)
            .await
    }

    /// 校验技能定义正确性。对应 TS `validateSkill`。
    pub async fn validate_skill(
        &self,
        skill_name: &str,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let body = serde_json::json!({ "skillName": skill_name }).to_string();
        let _: serde_json::Value = self
            .billing_post_body("/skill-generator/validate", Some(&body), signal)
            .await?;
        Ok(())
    }
}

/// 上传响应内层 `data.skill`。
#[derive(serde::Deserialize)]
struct UploadData {
    skill: SkillStoreItem,
}

/// `ApiResponse<T>` → `data`（公共端点；`code != 0` 抛 BusinessError）。
fn unwrap_public<T: Clone>(env: &ApiResponse<T>) -> Result<T> {
    if let Some(err) = env.business_error() {
        return Err(err);
    }
    Ok(env.data.clone())
}

/// 构造 skill-store 浏览 query string（对应 TS URLSearchParams 序列；仅非空键写入）。
fn build_skill_query(
    page: i64,
    page_size: i64,
    category: &str,
    keyword: &str,
    tag: &str,
    source: &str,
    minimal: bool,
) -> String {
    let mut parts: Vec<String> = vec![format!("page={page}"), format!("pageSize={page_size}")];
    if minimal {
        parts.push("fields=minimal".to_string());
    }
    if !category.is_empty() {
        parts.push(format!("category={}", urlencoding(category)));
    }
    if !keyword.is_empty() {
        parts.push(format!("keyword={}", urlencoding(keyword)));
    }
    if !tag.is_empty() {
        parts.push(format!("tag={}", urlencoding(tag)));
    }
    if !source.is_empty() {
        parts.push(format!("source={}", urlencoding(source)));
    }
    parts.join("&")
}

/// 从 `Content-Disposition` 头提取 filename（对应 TS 的简易解析；默认 `skill.zip`）。
fn parse_content_disposition_filename(headers: &reqwest::header::HeaderMap) -> String {
    let default = "skill.zip".to_string();
    let cd = match headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
    {
        Some(s) => s,
        None => return default,
    };
    let idx = match cd.find("filename") {
        Some(i) => i,
        None => return default,
    };
    let tail = &cd[idx..];
    // split('=', 2)：取 `=` 后第一段。
    let parts: Vec<&str> = tail.splitn(2, '=').collect();
    if parts.len() != 2 {
        return default;
    }
    let trimmed = parts[1]
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
    if trimmed.is_empty() {
        default
    } else {
        trimmed.to_string()
    }
}
