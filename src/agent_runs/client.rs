//! SDK-facing Agent Run Gateway 客户端。端口自 `agent-runs/client.ts`
//! （getter 子客户端 + 自有 requestRaw/requestAPI，不复用 chat 路由）。

use crate::agent_runs::remote_control::{
    is_terminal_remote_event, parse_remote_control_event, PermissionPolicy, RemoteControlEvent,
    RemotePermissionResultRequest, RemoteSessionTokenGrant, RemoteUserMessageAck,
    RemoteUserMessageRequest, WorkspacePolicy,
};
use crate::agent_runs::types::{
    AgentRun, AgentRunArtifact, AgentRunCreateRequest, AgentRunCreateResponse, AgentRunDownload,
    AgentRunErrorPayload, AgentRunListOptions, AgentRunListResult, AgentRunLocalToolResult,
    AgentRunRunOptions, AgentRunSettlement, AgentRunStatus, AgentRunStreamEvent,
    AgentRunStreamOptions, AgentRunUsage, AgentRunWithLocalToolsOptions,
    AGENT_RUN_RUNTIME_CRABCODE_REMOTE,
};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::core::http::{
    iter_sse_lines, parse_http_error_with_retry_after, read_limited, read_limited_text,
    DEFAULT_JSON_TIMEOUT_MS, MAX_DOWNLOAD_SIZE, MAX_ERROR_BODY_SIZE,
};
use crate::shared::{ApiResponse, Error, Result};
use futures::{Stream, StreamExt};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

/// SDK-facing cloud agent run gateway 子客户端。对应 TS `AgentRunsClient`。
///
/// 经 [`Client::agent_runs`] getter 获取（无状态，持 [`Client`] clone）。
pub struct AgentRunsClient {
    client: Client,
}

impl AgentRunsClient {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// 创建 agent run。对应 TS `create`（POST /agent-runs，retryOn401=false）。
    pub async fn create(
        &self,
        req: &AgentRunCreateRequest,
        signal: Option<CancellationToken>,
    ) -> Result<AgentRunCreateResponse> {
        let body = to_wire_create_request(req).to_string();
        let env: ApiResponse<WireAgentRunCreateResponse> = self
            .client
            .agent_runs_request_api("POST", "/agent-runs", Some(&body), signal, false)
            .await?;
        Ok(from_wire_create_response(env.data))
    }

    /// 列出调用者自己的 agent run（GET /agent-runs，Phase 5C 控制台；retryOn401=true）。对应 TS `list`。
    pub async fn list(
        &self,
        opts: &AgentRunListOptions,
        signal: Option<CancellationToken>,
    ) -> Result<AgentRunListResult> {
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(r) = &opts.runtime {
            params.push(("runtime".into(), r.clone()));
        }
        if let Some(s) = &opts.status {
            params.push(("status".into(), s.clone()));
        }
        if let Some(p) = opts.page {
            params.push(("page".into(), p.to_string()));
        }
        if let Some(ps) = opts.page_size {
            params.push(("page_size".into(), ps.to_string()));
        }
        let qs = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding(v)))
            .collect::<Vec<_>>()
            .join("&");
        let path = if qs.is_empty() {
            "/agent-runs".to_string()
        } else {
            format!("/agent-runs?{qs}")
        };
        let env: ApiResponse<WireAgentRunListResult> = self
            .client
            .agent_runs_request_api("GET", &path, None, signal, true)
            .await?;
        let resp = env.data;
        Ok(AgentRunListResult {
            records: resp
                .records
                .unwrap_or_default()
                .into_iter()
                .map(from_wire_run)
                .collect(),
            total: resp.total.unwrap_or(0),
            page: resp.page.unwrap_or(1),
            page_size: resp.page_size.unwrap_or(20),
        })
    }

    /// 查询单个 agent run（GET /agent-runs/:id；retryOn401=true）。对应 TS `get`。
    pub async fn get(&self, run_id: &str, signal: Option<CancellationToken>) -> Result<AgentRun> {
        let path = format!("/agent-runs/{}", urlencoding(run_id));
        let env: ApiResponse<WireAgentRun> = self
            .client
            .agent_runs_request_api("GET", &path, None, signal, true)
            .await?;
        Ok(from_wire_run(env.data))
    }

    /// 取消 agent run（POST /agent-runs/:id/cancel；retryOn401=false）。对应 TS `cancel`。
    pub async fn cancel(
        &self,
        run_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<AgentRun> {
        let path = format!("/agent-runs/{}/cancel", urlencoding(run_id));
        let env: ApiResponse<WireAgentRun> = self
            .client
            .agent_runs_request_api("POST", &path, Some("{}"), signal, false)
            .await?;
        Ok(from_wire_run(env.data))
    }

    /// 创建 CrabCode 远控 agent run（契约 §3，ADR-2 + ADR-5）。对应 TS `createRemoteRun`。
    ///
    /// 等价 `create()` 但 runtime 固定 `crabcode_remote` 且 runner + adapter 必填。
    /// 对应 stream 用 [`Self::stream_remote_control`]（事件 union 不同，契约 §4）。
    pub async fn create_remote_run(
        &self,
        req: &AgentRunCreateRequest,
        signal: Option<CancellationToken>,
    ) -> Result<AgentRunCreateResponse> {
        if req.runtime.as_deref() != Some(AGENT_RUN_RUNTIME_CRABCODE_REMOTE) {
            return Err(Error::other(
                "create_remote_run: runtime must be \"crabcode_remote\"",
            ));
        }
        if req.runner.is_none() {
            return Err(Error::other("create_remote_run: runner is required"));
        }
        if req.adapter.is_none() {
            return Err(Error::other("create_remote_run: adapter is required"));
        }
        self.create(req, signal).await
    }

    /// 提交 permission 决策（POST /agent-runs/:id/permission-results；契约 §4/§9/§14）。对应 TS `submitPermissionResult`。
    ///
    /// 仅 `approved` | `rejected` 可由客户端提交。需 remote_control scope（或 web JWT）。
    pub async fn submit_permission_result(
        &self,
        run_id: &str,
        result: &RemotePermissionResultRequest,
        signal: Option<CancellationToken>,
    ) -> Result<()> {
        let path = format!("/agent-runs/{}/permission-results", urlencoding(run_id));
        let body = json!({
            "request_id": result.request_id,
            "decision": result.decision,
            "reason": result.reason,
        })
        .to_string();
        // 返回 `{ok?}`，调用方不关心 → 解析后丢弃（空体亦容忍）。
        let _: Option<ApiResponse<Value>> = self
            .client
            .agent_runs_request_api_opt("POST", &path, Some(&body), signal, false)
            .await?;
        Ok(())
    }

    /// 会话中途追加用户消息（POST /agent-runs/:id/messages，Phase 5C）。对应 TS `submitUserMessage`。
    ///
    /// Role 由服务端硬编码 'user'（契约 §6 #5 防注入）; content ≤ 64KB。需 remote_control scope。
    pub async fn submit_user_message(
        &self,
        run_id: &str,
        message: &RemoteUserMessageRequest,
        signal: Option<CancellationToken>,
    ) -> Result<RemoteUserMessageAck> {
        let path = format!("/agent-runs/{}/messages", urlencoding(run_id));
        let body = json!({
            "request_id": message.request_id,
            "content": message.content,
        })
        .to_string();
        let env: Option<ApiResponse<WireUserMessageAck>> = self
            .client
            .agent_runs_request_api_opt("POST", &path, Some(&body), signal, false)
            .await?;
        let data = env.map(|e| e.data).unwrap_or_default();
        Ok(RemoteUserMessageAck {
            ok: data.ok.unwrap_or(false),
            request_id: data
                .request_id
                .filter(|s| !s.is_empty())
                .or_else(|| message.request_id.clone())
                .unwrap_or_default(),
        })
    }

    /// 揭示 desktop-runner run 的一次性 session token（POST /agent-runs/:id/remote-token，Phase 5B；契约 §18.1）。对应 TS `revealRemoteToken`。
    ///
    /// 仅 desktop launcher: token 一次性消费（第二次 409），永不落浏览器存储。需 remote_control scope。
    pub async fn reveal_remote_token(
        &self,
        run_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<RemoteSessionTokenGrant> {
        let path = format!("/agent-runs/{}/remote-token", urlencoding(run_id));
        let env: ApiResponse<WireRemoteSessionTokenGrant> = self
            .client
            .agent_runs_request_api("POST", &path, Some("{}"), signal, false)
            .await?;
        let data = env.data;
        Ok(RemoteSessionTokenGrant {
            access_token: data.access_token.unwrap_or_default(),
            session_url: data.session_url.unwrap_or_default(),
            tenant_id: data.tenant_id.unwrap_or_default(),
            workspace: data.workspace.filter(|s| !s.is_empty()),
        })
    }

    /// 列出 run 的产物（GET /agent-runs/:id/artifacts；retryOn401=true）。对应 TS `listArtifacts`。
    pub async fn list_artifacts(
        &self,
        run_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<AgentRunArtifact>> {
        let path = format!("/agent-runs/{}/artifacts", urlencoding(run_id));
        let env: ApiResponse<WireAgentRunArtifactList> = self
            .client
            .agent_runs_request_api("GET", &path, None, signal, true)
            .await?;
        Ok(env
            .data
            .artifacts
            .unwrap_or_default()
            .into_iter()
            .map(from_wire_artifact)
            .collect())
    }

    /// 提交本地工具结果（POST /agent-runs/:id/local-tool-results；retryOn401=false）。对应 TS `submitLocalToolResult`。
    pub async fn submit_local_tool_result(
        &self,
        run_id: &str,
        result: &AgentRunLocalToolResult,
        signal: Option<CancellationToken>,
    ) -> Result<AgentRun> {
        let path = format!("/agent-runs/{}/local-tool-results", urlencoding(run_id));
        let body = json!({
            "request_id": result.request_id,
            "ok": result.ok,
            "content": result.content,
            "error": result.error,
        })
        .to_string();
        let env: ApiResponse<WireAgentRun> = self
            .client
            .agent_runs_request_api("POST", &path, Some(&body), signal, false)
            .await?;
        Ok(from_wire_run(env.data))
    }

    /// 下载产物（GET /agent-runs/:id/artifacts/:artifactId；retryOn401=true）。对应 TS `downloadArtifact`。
    ///
    /// 🔴 v2.5.0：多读 1 字节探测超限 —— 超过 [`MAX_DOWNLOAD_SIZE`] **抛错**，
    /// 绝不静默截断（否则下游拿到被砍断的不完整产物却毫无感知）。
    pub async fn download_artifact(
        &self,
        run_id: &str,
        artifact_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<AgentRunDownload> {
        let path = format!(
            "/agent-runs/{}/artifacts/{}",
            urlencoding(run_id),
            urlencoding(artifact_id)
        );
        let resp = self
            .client
            .agent_runs_request_raw(
                "GET",
                &path,
                None,
                signal.as_ref(),
                true,
                "application/json",
            )
            .await?;
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let filename = filename_from_content_disposition(
            resp.headers()
                .get(reqwest::header::CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok()),
        )
        .unwrap_or_else(|| artifact_id.to_string());

        // 多读 1 字节探测超限（与 download_skill 一致），超限抛错不截断。
        let data = read_limited(resp.bytes_stream(), MAX_DOWNLOAD_SIZE + 1).await?;
        if data.len() > MAX_DOWNLOAD_SIZE {
            return Err(Error::other(format!(
                "download artifact: response exceeds {}MB limit",
                MAX_DOWNLOAD_SIZE >> 20
            )));
        }
        Ok(AgentRunDownload {
            data,
            filename,
            content_type,
        })
    }

    /// 流式 agent run 事件（GET /agent-runs/:id/stream）。对应 TS `stream`。
    ///
    /// 🔴 计费安全：流式只走单次 `do_request`，**绝不重试**（仅 401 单次 force_refresh）。
    /// `error` 事件在 `throw_on_error`（默认 true）时转 [`Error::AgentRunStream`] 抛出。
    pub fn stream(
        &self,
        run_id: &str,
        opts: AgentRunStreamOptions,
        signal: Option<CancellationToken>,
    ) -> impl Stream<Item = Result<AgentRunStreamEvent>> + '_ {
        let run_id = run_id.to_string();
        async_stream::try_stream! {
            let inner = self.stream_gen(&run_id, signal);
            futures::pin_mut!(inner);
            while let Some(ev) = inner.next().await {
                let ev = ev?;
                if matches!(ev, AgentRunStreamEvent::Error { .. }) && opts.throw_on_error {
                    if let AgentRunStreamEvent::Error { error } = &ev {
                        Err(Error::AgentRunStream {
                            code: error.code.clone().unwrap_or_default(),
                            stage: error.stage.clone(),
                            message: error.message.clone(),
                            retryable: error.retryable.unwrap_or(false),
                        })?;
                    }
                }
                yield ev;
            }
        }
    }

    /// 流式 CrabCode 远控事件（契约 §4，11-event union）。对应 TS `streamRemoteControl`。
    ///
    /// 🔴 未知 event type / 畸形帧 **静默丢弃**（对齐 `parse_remote_control_event` 的 None 契约；
    /// 与 [`Self::stream`] 的未知→Error 注入流**相反**）。`done`/`settle` 终结事件后自然结束，从不抛异常。
    pub fn stream_remote_control(
        &self,
        run_id: &str,
        signal: Option<CancellationToken>,
    ) -> impl Stream<Item = Result<RemoteControlEvent>> + '_ {
        let run_id = run_id.to_string();
        async_stream::try_stream! {
            let frames = self.remote_sse_frames(&run_id, signal);
            futures::pin_mut!(frames);
            while let Some(frame) = frames.next().await {
                let frame = frame?;
                // 🔴 未知 type / 缺字段 → None 静默丢弃。
                if let Some(ev) = parse_remote_control_event(&frame) {
                    let terminal = is_terminal_remote_event(&ev);
                    yield ev;
                    if terminal {
                        return;
                    }
                }
            }
        }
    }

    /// create + stream 一气呵成。对应 TS `run`。
    pub fn run(
        &self,
        req: &AgentRunCreateRequest,
        opts: AgentRunRunOptions,
        signal: Option<CancellationToken>,
    ) -> impl Stream<Item = Result<AgentRunStreamEvent>> + '_ {
        let req = req.clone();
        async_stream::try_stream! {
            let created = self.create(&req, signal.clone()).await?;
            let inner = self.stream(&created.run_id, opts, signal);
            futures::pin_mut!(inner);
            while let Some(ev) = inner.next().await {
                yield ev?;
            }
        }
    }

    /// run + 本地工具回调编排（local_tool_request → handler → submit_local_tool_result）。对应 TS `runWithLocalTools`。
    ///
    /// 🔴 v2.5.0 本地工具回调硬超时：即便 handler 完全忽略 signal（不响应协作式取消），
    /// `timeout_ms` 后也会胜出返回稳定失败结果，不会永挂（`tokio::select!` 双臂）。
    pub fn run_with_local_tools<H, Fut>(
        &self,
        req: &AgentRunCreateRequest,
        handlers: HashMap<String, H>,
        opts: AgentRunWithLocalToolsOptions,
        signal: Option<CancellationToken>,
    ) -> impl Stream<Item = Result<AgentRunStreamEvent>> + '_
    where
        H: Fn(Value, LocalToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value>> + Send + 'static,
    {
        let req = req.clone();
        let stream_opts = AgentRunStreamOptions {
            throw_on_error: opts.throw_on_error,
        };
        async_stream::try_stream! {
            let mut current_run_id = String::new();
            let run = self.run(&req, stream_opts, signal.clone());
            futures::pin_mut!(run);
            while let Some(ev) = run.next().await {
                let ev = ev?;
                if let AgentRunStreamEvent::RunStarted { run_id, .. } = &ev {
                    current_run_id = run_id.clone();
                }

                if let AgentRunStreamEvent::LocalToolRequest { request_id, name, input } = &ev {
                    if current_run_id.is_empty() {
                        Err(Error::other("local tool request arrived before run_started"))?;
                    }
                    let result = invoke_local_tool(
                        &current_run_id,
                        request_id,
                        name,
                        input.clone(),
                        &handlers,
                        opts.timeout_ms,
                        signal.as_ref(),
                    )
                    .await;
                    // 先 yield 事件再回写（对齐 TS yield 顺序）。
                    let run_id_for_submit = current_run_id.clone();
                    yield ev;
                    self.submit_local_tool_result(&run_id_for_submit, &result, signal.clone()).await?;
                    continue;
                }

                yield ev;
            }
        }
    }

    // ===========================================================================
    // 内部：SSE 解析（agent run events / remote control frames）
    // ===========================================================================

    /// agent run SSE 流的真实实现（401 单次 force_refresh 重试经 [`Client::agent_runs_request_raw`]）。
    fn stream_gen<'a>(
        &'a self,
        run_id: &'a str,
        signal: Option<CancellationToken>,
    ) -> Pin<Box<dyn Stream<Item = Result<AgentRunStreamEvent>> + Send + 'a>> {
        Box::pin(async_stream::try_stream! {
            let path = format!("/agent-runs/{}/stream", urlencoding(run_id));
            let resp = self
                .client
                .agent_runs_request_raw("GET", &path, None, signal.as_ref(), true, "text/event-stream")
                .await?;

            let lines = iter_sse_lines(resp.bytes_stream());
            futures::pin_mut!(lines);
            let mut event_name = String::new();
            let mut data_lines: Vec<String> = Vec::new();
            loop {
                let next: Option<Result<String>> = match &signal {
                    Some(cancel) => tokio::select! {
                        l = lines.next() => l,
                        _ = cancel.cancelled() => Some(Err(Error::other("agent run stream: aborted"))),
                    },
                    None => lines.next().await,
                };
                let line = match next {
                    Some(l) => l?,
                    None => break,
                };
                if line.is_empty() {
                    if let Some(ev) = flush_agent_run_event(&event_name, &mut data_lines) {
                        event_name.clear();
                        yield ev;
                    } else {
                        event_name.clear();
                    }
                    continue;
                }
                if line.starts_with(':') {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("event:") {
                    event_name = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_string());
                }
            }
            if let Some(ev) = flush_agent_run_event(&event_name, &mut data_lines) {
                yield ev;
            }
        })
    }

    /// remote-control SSE 帧流（产出原始 JSON 对象，注入 `__event__`/`type` 名）。对应 TS `readAgentRunSSEFrames`。
    fn remote_sse_frames<'a>(
        &'a self,
        run_id: &'a str,
        signal: Option<CancellationToken>,
    ) -> Pin<Box<dyn Stream<Item = Result<Value>> + Send + 'a>> {
        Box::pin(async_stream::try_stream! {
            let path = format!("/agent-runs/{}/stream", urlencoding(run_id));
            let resp = self
                .client
                .agent_runs_request_raw("GET", &path, None, signal.as_ref(), true, "text/event-stream")
                .await?;

            let lines = iter_sse_lines(resp.bytes_stream());
            futures::pin_mut!(lines);
            let mut event_name = String::new();
            let mut data_lines: Vec<String> = Vec::new();
            loop {
                let next: Option<Result<String>> = match &signal {
                    Some(cancel) => tokio::select! {
                        l = lines.next() => l,
                        _ = cancel.cancelled() => Some(Err(Error::other("remote-control stream: aborted"))),
                    },
                    None => lines.next().await,
                };
                let line = match next {
                    Some(l) => l?,
                    None => break,
                };
                if line.is_empty() {
                    if let Some(frame) = flush_remote_frame(&event_name, &mut data_lines) {
                        event_name.clear();
                        yield frame;
                    } else {
                        event_name.clear();
                    }
                    continue;
                }
                if line.starts_with(':') {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("event:") {
                    event_name = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_string());
                }
            }
            if let Some(frame) = flush_remote_frame(&event_name, &mut data_lines) {
                yield frame;
            }
        })
    }
}

impl Client {
    /// SDK-facing cloud agent run gateway。对应 TS `client.agentRuns` getter。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use acosmi::Client;
    /// # use acosmi::agent_runs::AgentRunCreateRequest;
    /// # async fn demo(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    /// let runs = client.agent_runs();
    /// let run = runs
    ///     .create(
    ///         &AgentRunCreateRequest {
    ///             app_id: "crabdesign".into(),
    ///             input: "Create a landing page mockup".into(),
    ///             ..Default::default()
    ///         },
    ///         None,
    ///     )
    ///     .await?;
    /// println!("run_id = {}", run.run_id);
    /// # Ok(()) }
    /// ```
    pub fn agent_runs(&self) -> AgentRunsClient {
        AgentRunsClient::new(self.clone())
    }

    /// agent-runs/byok 域内部：非流式 JSON 请求 → `ApiResponse<T>`（业务码检查在调用方读 .data 前自动）。
    /// 对应 TS `AgentRunsClient.requestAPI`（空体 → 强 Err；调用方要容忍空体用 `_opt`）。
    pub(crate) async fn agent_runs_request_api<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
        retry_on_401: bool,
    ) -> Result<T> {
        let opt = self
            .agent_runs_request_api_opt::<T>(method, path, body, signal, retry_on_401)
            .await?;
        opt.ok_or_else(|| Error::other(format!("{path}: empty response body")))
    }

    /// 同 [`Self::agent_runs_request_api`] 但空体返回 `Ok(None)`（对齐 TS `requestAPI` 空体→undefined）。
    pub(crate) async fn agent_runs_request_api_opt<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        signal: Option<CancellationToken>,
        retry_on_401: bool,
    ) -> Result<Option<T>> {
        // 非流式 JSON 套默认超时（对应 TS requestAPI timeoutMs ?? DEFAULT_API_TIMEOUT_MS）。
        let ctl = self.derive_timeout_token(DEFAULT_JSON_TIMEOUT_MS, signal);
        let resp = self
            .agent_runs_request_raw(
                method,
                path,
                body,
                ctl.as_ref(),
                retry_on_401,
                "application/json",
            )
            .await?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::other(format!("{path}: read body: {e}")))?;
        if bytes.is_empty() {
            return Ok(None);
        }
        let v: T = serde_json::from_slice(&bytes)
            .map_err(|e| Error::other(format!("{path}: decode: {e}")))?;
        Ok(Some(v))
    }

    /// agent-runs 域自有 raw 请求（对应 TS `AgentRunsClient.requestRaw` → `requestRawInner`）。
    ///
    /// ensure_token → Bearer + Accept → **单次 [`Client::do_request`]（绝不重试，流式安全）** →
    /// 401（且 `retry_on_401`）单次 force_refresh 重试（`retried` guard 防递归）→ 非 2xx 抛 [`Error::Http`]。
    /// 返回 raw `reqwest::Response`（供 SSE / 下载流式读 body）。
    pub(crate) fn agent_runs_request_raw<'a>(
        &'a self,
        method: &'a str,
        path: &'a str,
        body: Option<&'a str>,
        signal: Option<&'a CancellationToken>,
        retry_on_401: bool,
        accept: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<reqwest::Response>> + Send + 'a>> {
        Box::pin(self.agent_runs_request_raw_inner(
            method,
            path,
            body,
            signal,
            retry_on_401,
            accept,
            false,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn agent_runs_request_raw_inner(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        signal: Option<&CancellationToken>,
        retry_on_401: bool,
        accept: &str,
        retried: bool,
    ) -> Result<reqwest::Response> {
        let token = self.ensure_token(signal.cloned()).await?;
        let url = self.api_url(path);
        let m = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|e| Error::other(format!("invalid method {method}: {e}")))?;

        let mut headers: Vec<(reqwest::header::HeaderName, String)> = vec![
            (reqwest::header::AUTHORIZATION, format!("Bearer {token}")),
            (reqwest::header::ACCEPT, accept.to_string()),
        ];
        if body.is_some() {
            headers.push((
                reqwest::header::CONTENT_TYPE,
                "application/json".to_string(),
            ));
        }

        // 🔴 流式安全：只走单次 do_request（绝不 do_request_with_retry）。
        let resp = self
            .do_request(m.clone(), &url, &headers, body, signal)
            .await?;

        if resp.status().as_u16() == 401 && retry_on_401 && !retried {
            drop(resp);
            self.force_refresh(signal.cloned())
                .await
                .map_err(|e| Error::other(format!("unauthorized and refresh failed: {e}")))?;
            return Box::pin(self.agent_runs_request_raw_inner(
                method,
                path,
                body,
                signal,
                retry_on_401,
                accept,
                true,
            ))
            .await;
        }

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let retry_after = parse_retry_after_secs(resp.headers());
            let text = read_limited_text(resp.bytes_stream(), MAX_ERROR_BODY_SIZE).await?;
            return Err(Error::Http(parse_http_error_with_retry_after(
                status,
                &text,
                retry_after,
            )));
        }
        Ok(resp)
    }
}

/// 本地工具 handler 上下文。对应 TS `AgentRunLocalToolHandlerContext`。
#[derive(Debug, Clone)]
pub struct LocalToolContext {
    pub run_id: String,
    pub request_id: String,
    pub name: String,
    /// 协作式取消信号；硬超时触发时 cancel（handler 可监听尽早退出）。
    pub signal: CancellationToken,
}

/// 调用本地工具 handler，带硬超时（对应 TS `invokeLocalTool`）。
///
/// 无 handler → 立即返回 rejected；有 handler → `tokio::select!` 双臂：handler vs 超时定时器，
/// **超时方胜出立即返回稳定失败结果**（即便 handler 永挂也不阻塞）。
#[allow(clippy::too_many_arguments)]
async fn invoke_local_tool<H, Fut>(
    run_id: &str,
    request_id: &str,
    name: &str,
    input: Value,
    handlers: &HashMap<String, H>,
    timeout_ms: u64,
    parent: Option<&CancellationToken>,
) -> AgentRunLocalToolResult
where
    H: Fn(Value, LocalToolContext) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let handler = match handlers.get(name) {
        Some(h) => h,
        None => {
            return AgentRunLocalToolResult {
                request_id: request_id.to_string(),
                ok: false,
                content: None,
                error: Some(format!("local tool rejected: no handler for {name}")),
            };
        }
    };

    let ctl = CancellationToken::new();
    // parent 取消 → 联动 ctl（协作式取消提示）。
    if let Some(p) = parent {
        if p.is_cancelled() {
            ctl.cancel();
        } else {
            let child = ctl.clone();
            let parent = p.clone();
            tokio::spawn(async move {
                parent.cancelled().await;
                child.cancel();
            });
        }
    }

    let ctx = LocalToolContext {
        run_id: run_id.to_string(),
        request_id: request_id.to_string(),
        name: name.to_string(),
        signal: ctl.clone(),
    };
    let handler_fut = handler(input, ctx);
    let timeout = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms));

    tokio::select! {
        biased;
        // handler 先完成。
        out = handler_fut => match out {
            Ok(content) => AgentRunLocalToolResult {
                request_id: request_id.to_string(),
                ok: true,
                content: Some(content),
                error: None,
            },
            Err(e) => {
                // parent 取消时直接传播失败原因；否则当作 handler 失败。
                let timed_out = ctl.is_cancelled() && parent.map(|p| !p.is_cancelled()).unwrap_or(true);
                AgentRunLocalToolResult {
                    request_id: request_id.to_string(),
                    ok: false,
                    content: None,
                    error: Some(if timed_out {
                        format!("local tool timed out after {timeout_ms}ms")
                    } else {
                        e.to_string()
                    }),
                }
            }
        },
        // 超时胜出：cancel ctl（协作式提示）并返回稳定失败结果。
        _ = timeout => {
            ctl.cancel();
            AgentRunLocalToolResult {
                request_id: request_id.to_string(),
                ok: false,
                content: None,
                error: Some(format!("local tool timed out after {timeout_ms}ms")),
            }
        }
    }
}

// =============================================================================
// SSE flush helpers
// =============================================================================

/// flush 累积的 agent-run SSE 数据行为一个事件（`[DONE]` → None）。对应 TS `readAgentRunEvents` flush。
fn flush_agent_run_event(
    event_name: &str,
    data_lines: &mut Vec<String>,
) -> Option<AgentRunStreamEvent> {
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    data_lines.clear();
    if data == "[DONE]" {
        return None;
    }
    Some(parse_agent_run_event(event_name, &data))
}

/// flush 累积的 remote-control SSE 数据行为一个原始 JSON 帧（注入 type=event_name）。对应 TS `readAgentRunSSEFrames` flush。
fn flush_remote_frame(event_name: &str, data_lines: &mut Vec<String>) -> Option<Value> {
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    data_lines.clear();
    if data == "[DONE]" {
        return None;
    }
    let parsed: Value = serde_json::from_str(&data).ok()?;
    let mut obj = match parsed {
        Value::Object(m) => m,
        _ => return None,
    };
    if !obj.contains_key("type") && !event_name.is_empty() {
        obj.insert("type".to_string(), Value::String(event_name.to_string()));
    }
    Some(Value::Object(obj))
}

/// 解析单个 agent-run SSE 事件。对应 TS `parseAgentRunEvent`。
///
/// 🔴 **未知 type → [`AgentRunStreamEvent::Error`]（code=`unknown_event`）注入流**（不丢；
/// 与 `parse_remote_control_event` 未知→None 相反）。JSON 解析失败 → 同样兜底为 unknown error。
fn parse_agent_run_event(event_name: &str, data: &str) -> AgentRunStreamEvent {
    let payload: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => {
            // 非 JSON：当作 {type: eventName, data: 原文} 处理 → 走未知分支兜底 error。
            return unknown_event(event_name, &json!({ "type": event_name, "data": data }));
        }
    };
    let obj: serde_json::Map<String, Value> = match &payload {
        Value::Object(m) => m.clone(),
        other => {
            let mut m = serde_json::Map::new();
            m.insert("type".to_string(), Value::String(event_name.to_string()));
            m.insert("data".to_string(), other.clone());
            m
        }
    };
    let type_ = sf(&obj, &["type"]);
    let type_ = if type_.is_empty() {
        event_name.to_string()
    } else {
        type_
    };

    match type_.as_str() {
        "run_started" => AgentRunStreamEvent::RunStarted {
            run_id: sf(&obj, &["run_id", "runId"]),
            session_id: sf(&obj, &["session_id", "sessionId"]),
        },
        "status" => AgentRunStreamEvent::Status {
            status: sf(&obj, &["status"]),
            message: osf(&obj, &["message"]),
        },
        "text_delta" => AgentRunStreamEvent::TextDelta {
            text: sf(&obj, &["text"]),
        },
        "reasoning_delta" => AgentRunStreamEvent::ReasoningDelta {
            text: sf(&obj, &["text"]),
        },
        "tool_call" => AgentRunStreamEvent::ToolCall {
            id: sf(&obj, &["id"]),
            name: sf(&obj, &["name"]),
            input: obj.get("input").cloned(),
        },
        "tool_result" => AgentRunStreamEvent::ToolResult {
            id: sf(&obj, &["id"]),
            name: osf(&obj, &["name"]),
            result: obj.get("result").cloned(),
            error: osf(&obj, &["error"]),
        },
        "local_tool_request" => AgentRunStreamEvent::LocalToolRequest {
            request_id: sf(&obj, &["request_id", "requestId"]),
            name: sf(&obj, &["name"]),
            input: obj.get("input").cloned().unwrap_or(Value::Null),
        },
        "artifact" => {
            let art_val = obj
                .get("artifact")
                .filter(|v| v.is_object())
                .cloned()
                .unwrap_or_else(|| Value::Object(obj.clone()));
            AgentRunStreamEvent::Artifact {
                artifact: from_wire_artifact_value(&art_val),
            }
        }
        "sources" => AgentRunStreamEvent::Sources {
            sources: obj.get("sources").cloned().unwrap_or(Value::Null),
        },
        "usage" => {
            let src = obj
                .get("usage")
                .filter(|v| v.is_object())
                .cloned()
                .unwrap_or_else(|| Value::Object(obj.clone()));
            AgentRunStreamEvent::Usage {
                usage: normalize_usage(&src),
            }
        }
        "settle" => {
            let src = obj
                .get("settlement")
                .filter(|v| v.is_object())
                .cloned()
                .unwrap_or_else(|| Value::Object(obj.clone()));
            AgentRunStreamEvent::Settle {
                settlement: normalize_settlement(&src),
            }
        }
        "error" => {
            let err = normalize_error(obj.get("error"))
                .or_else(|| normalize_error(Some(&Value::Object(obj.clone()))))
                .unwrap_or_else(|| AgentRunErrorPayload {
                    message: "agent run failed".to_string(),
                    ..Default::default()
                });
            AgentRunStreamEvent::Error { error: err }
        }
        "done" => AgentRunStreamEvent::Done {
            run_id: sf(&obj, &["run_id", "runId"]),
            status: sf(&obj, &["status"]),
        },
        // 🔴 未知 type → Error 注入流（不丢）。
        _ => unknown_event(&type_, &Value::Object(obj)),
    }
}

fn unknown_event(type_: &str, raw: &Value) -> AgentRunStreamEvent {
    AgentRunStreamEvent::Error {
        error: AgentRunErrorPayload {
            code: Some("unknown_event".to_string()),
            message: format!("unknown agent run event: {type_}"),
            stage: None,
            retryable: None,
            raw: Some(raw.clone()),
        },
    }
}

// =============================================================================
// usage / settlement / error 归一化
// =============================================================================

fn normalize_usage(value: &Value) -> AgentRunUsage {
    let obj = value.as_object().cloned().unwrap_or_default();
    AgentRunUsage {
        input_tokens: nf(&obj, &["input_tokens", "inputTokens"]),
        output_tokens: nf(&obj, &["output_tokens", "outputTokens"]),
        total_tokens: nf(&obj, &["total_tokens", "totalTokens"]),
        cache_read_tokens: nf(&obj, &["cache_read_tokens", "cacheReadTokens"]),
        cache_create_tokens: nf(&obj, &["cache_create_tokens", "cacheCreateTokens"]),
        exact: bf(&obj, &["exact"]),
        source: osf(&obj, &["source"]),
        raw: value.clone(),
    }
}

fn normalize_settlement(value: &Value) -> AgentRunSettlement {
    let obj = value.as_object().cloned().unwrap_or_default();
    AgentRunSettlement {
        request_id: osf(&obj, &["request_id", "requestId"]),
        status: osf(&obj, &["status"]),
        consume_status: osf(&obj, &["consume_status", "consumeStatus"]),
        input_tokens: nf(&obj, &["input_tokens", "inputTokens"]),
        output_tokens: nf(&obj, &["output_tokens", "outputTokens"]),
        total_tokens: nf(&obj, &["total_tokens", "totalTokens"]),
        cache_read_tokens: nf(&obj, &["cache_read_tokens", "cacheReadTokens"]),
        cache_create_tokens: nf(&obj, &["cache_create_tokens", "cacheCreateTokens"]),
        token_remaining: nf(&obj, &["token_remaining", "tokenRemaining"]),
        call_remaining: nf(&obj, &["call_remaining", "callRemaining"]),
        retry_queued: bf(&obj, &["retry_queued", "retryQueued"]),
        exact: bf(&obj, &["exact"]),
        raw: value.clone(),
    }
}

fn normalize_error(value: Option<&Value>) -> Option<AgentRunErrorPayload> {
    let value = value?;
    match value {
        Value::Null => None,
        Value::String(s) => Some(AgentRunErrorPayload {
            message: s.clone(),
            raw: Some(value.clone()),
            ..Default::default()
        }),
        Value::Object(obj) => {
            let message = {
                let m = sf(obj, &["message", "error"]);
                if m.is_empty() {
                    "agent run failed".to_string()
                } else {
                    m
                }
            };
            Some(AgentRunErrorPayload {
                code: osf(obj, &["code", "error_code", "errorCode"]),
                message,
                stage: osf(obj, &["stage"]),
                retryable: obj.get("retryable").and_then(|v| v.as_bool()),
                raw: Some(value.clone()),
            })
        }
        other => Some(AgentRunErrorPayload {
            message: other.to_string(),
            raw: Some(value.clone()),
            ..Default::default()
        }),
    }
}

// =============================================================================
// field 提取 helpers（对齐 TS stringField/optionalStringField/numberField/booleanField）
// =============================================================================

fn sf(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = obj.get(*k).and_then(|v| v.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}

fn osf(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    let s = sf(obj, keys);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn nf(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            if let Some(i) = v.as_i64() {
                return Some(i);
            }
            if let Some(f) = v.as_f64().filter(|f| f.is_finite()) {
                return Some(f as i64);
            }
        }
    }
    None
}

fn bf(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for k in keys {
        if let Some(b) = obj.get(*k).and_then(|v| v.as_bool()) {
            return Some(b);
        }
    }
    None
}

// =============================================================================
// wire DTO（JSON 反序列化） + 转换
// =============================================================================

#[derive(serde::Deserialize, Default)]
struct WireAgentRunCreateResponse {
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct WireAgentRun {
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    metadata: Option<HashMap<String, String>>,
    #[serde(default)]
    runtime: Option<String>,
    #[serde(default)]
    runner: Option<String>,
    #[serde(default)]
    adapter: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct WireAgentRunListResult {
    #[serde(default)]
    records: Option<Vec<WireAgentRun>>,
    #[serde(default)]
    total: Option<i64>,
    #[serde(default)]
    page: Option<i64>,
    // 注: 外层 pageSize 是 camelCase（对齐 listConsumeRecords 信封）。
    #[serde(default, rename = "pageSize")]
    page_size: Option<i64>,
}

#[derive(serde::Deserialize, Default)]
struct WireRemoteSessionTokenGrant {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    session_url: Option<String>,
    #[serde(default)]
    tenant_id: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct WireUserMessageAck {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct WireAgentRunArtifactList {
    #[serde(default)]
    artifacts: Option<Vec<Value>>,
}

fn to_wire_create_request(req: &AgentRunCreateRequest) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("app_id".into(), json!(req.app_id));
    insert_opt(&mut m, "mode", req.mode.as_ref());
    insert_opt(&mut m, "session_id", req.session_id.as_ref());
    m.insert("input".into(), json!(req.input));
    if let Some(msgs) = &req.messages {
        m.insert("messages".into(), json!(msgs));
    }
    insert_opt(&mut m, "model", req.model.as_ref());
    if let Some(v) = &req.active_skill_ids {
        m.insert("active_skill_ids".into(), json!(v));
    }
    if let Some(v) = &req.knowledge_base_ids {
        m.insert("knowledge_base_ids".into(), json!(v));
    }
    if let Some(md) = &req.metadata {
        m.insert("metadata".into(), json!(md));
    }
    if let Some(p) = &req.local_context_policy {
        m.insert(
            "local_context_policy".into(),
            json!({
                "enabled": p.enabled,
                "readonly": p.readonly,
                "max_bytes": p.max_bytes,
                "allowed_tools": p.allowed_tools,
            }),
        );
    }
    insert_opt(&mut m, "runtime", req.runtime.as_ref());
    if let Some(r) = &req.runner {
        m.insert("runner".into(), json!(r.as_str()));
    }
    if let Some(a) = &req.adapter {
        m.insert("adapter".into(), json!(a.as_str()));
    }
    if let Some(p) = &req.permission_policy {
        m.insert("permission_policy".into(), to_wire_permission_policy(p));
    }
    if let Some(p) = &req.workspace_policy {
        m.insert("workspace_policy".into(), to_wire_workspace_policy(p));
    }
    insert_opt(
        &mut m,
        "byok_credential_ref",
        req.byok_credential_ref.as_ref(),
    );
    if let Some(p) = &req.artifact_policy {
        m.insert(
            "artifact_policy".into(),
            json!({ "enabled": p.enabled, "max_files": p.max_files }),
        );
    }
    Value::Object(m)
}

fn to_wire_permission_policy(p: &PermissionPolicy) -> Value {
    json!({
        "shell_allowed": p.shell_allowed,
        "shell_deny_list": p.shell_deny_list,
        "network_allowed": p.network_allowed,
        "write_allowed": p.write_allowed,
        "approval_timeout_ms": p.approval_timeout_ms,
        "required_actors": p.required_actors,
    })
}

fn to_wire_workspace_policy(p: &WorkspacePolicy) -> Value {
    json!({
        "read_only": p.read_only,
        "allowed_paths": p.allowed_paths,
        "denied_paths": p.denied_paths,
        "max_bytes": p.max_bytes,
    })
}

fn insert_opt(m: &mut serde_json::Map<String, Value>, key: &str, v: Option<&String>) {
    if let Some(v) = v {
        m.insert(key.into(), json!(v));
    }
}

fn from_wire_create_response(resp: WireAgentRunCreateResponse) -> AgentRunCreateResponse {
    AgentRunCreateResponse {
        run_id: resp.run_id.unwrap_or_default(),
        session_id: resp.session_id.unwrap_or_default(),
        status: AgentRunStatus::from_wire(resp.status.as_deref()),
    }
}

fn from_wire_run(resp: WireAgentRun) -> AgentRun {
    AgentRun {
        run_id: resp.run_id.unwrap_or_default(),
        session_id: resp.session_id.unwrap_or_default(),
        app_id: resp.app_id,
        mode: resp.mode,
        status: Some(AgentRunStatus::from_wire(resp.status.as_deref())),
        created_at: resp.created_at,
        started_at: resp.started_at,
        completed_at: resp.completed_at,
        error: normalize_error(resp.error.as_ref()),
        metadata: resp.metadata,
        runtime: resp.runtime,
        runner: resp.runner,
        adapter: resp.adapter,
    }
}

/// 从 wire artifact Value（列表/事件共用，宽容多字段）构建 [`AgentRunArtifact`]。对应 TS `fromWireArtifact`。
fn from_wire_artifact_value(v: &Value) -> AgentRunArtifact {
    let obj = v.as_object().cloned().unwrap_or_default();
    let id = sf(&obj, &["id", "artifact_id"]);
    let filename = {
        let f = sf(&obj, &["filename", "name", "id", "artifact_id"]);
        if f.is_empty() {
            "artifact".to_string()
        } else {
            f
        }
    };
    AgentRunArtifact {
        id,
        filename,
        content_type: osf(&obj, &["content_type", "mime_type"]),
        size: nf(&obj, &["size"]),
        r#type: osf(&obj, &["type"]),
        metadata: obj
            .get("metadata")
            .and_then(|m| serde_json::from_value::<HashMap<String, String>>(m.clone()).ok()),
    }
}

fn from_wire_artifact(v: Value) -> AgentRunArtifact {
    from_wire_artifact_value(&v)
}

/// 从 Content-Disposition 解析 filename（UTF-8'' 优先，回退 plain）。对应 TS `filenameFromContentDisposition`。
fn filename_from_content_disposition(value: Option<&str>) -> Option<String> {
    let value = value?;
    // filename*=UTF-8''<encoded>
    if let Some(idx) = value.to_lowercase().find("filename*=utf-8''") {
        let start = idx + "filename*=utf-8''".len();
        let rest = &value[start..];
        let encoded = rest.split(';').next().unwrap_or("").trim();
        if !encoded.is_empty() {
            return Some(percent_decode(encoded));
        }
    }
    // filename="..." / filename=...
    let lower = value.to_lowercase();
    if let Some(idx) = lower.find("filename=") {
        let start = idx + "filename=".len();
        let rest = &value[start..];
        let raw = rest.split(';').next().unwrap_or("").trim();
        let raw = raw.trim_matches('"');
        if !raw.is_empty() {
            return Some(raw.to_string());
        }
    }
    None
}

/// 最小 percent-decode（用于 Content-Disposition filename* 的 UTF-8'' 段）。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// `parse_retry_after_secs` 仅 client.rs 内私有；这里复刻最小版（agent-runs 自有 raw 路径用）。
fn parse_retry_after_secs(headers: &reqwest::header::HeaderMap) -> i64 {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&s| s > 0)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::remote_control::{
        is_terminal_remote_event, parse_remote_control_event, RemoteControlEvent,
    };

    // === 🔴 红线 1a: AgentRunStreamEvent 未知 type → Error 注入流（不丢）===
    #[test]
    fn agent_run_unknown_event_becomes_error() {
        let ev = parse_agent_run_event("", r#"{"type":"totally_new_event","foo":1}"#);
        match ev {
            AgentRunStreamEvent::Error { error } => {
                assert_eq!(error.code.as_deref(), Some("unknown_event"));
                assert!(error.message.contains("totally_new_event"));
                assert!(error.raw.is_some());
            }
            other => panic!("expected Error, got {}", other.type_str()),
        }
    }

    #[test]
    fn agent_run_unknown_event_from_event_name_fallback() {
        // payload 无 type，靠 SSE event: 名兜底；event 名也未知 → Error。
        let ev = parse_agent_run_event("mystery", r#"{"foo":1}"#);
        assert!(matches!(ev, AgentRunStreamEvent::Error { .. }));
        if let AgentRunStreamEvent::Error { error } = ev {
            assert!(error.message.contains("mystery"));
        }
    }

    #[test]
    fn agent_run_malformed_json_becomes_error() {
        let ev = parse_agent_run_event("text_delta", "not json {{{");
        assert!(matches!(ev, AgentRunStreamEvent::Error { .. }));
    }

    #[test]
    fn agent_run_known_events_parse() {
        let rs = parse_agent_run_event(
            "",
            r#"{"type":"run_started","run_id":"r1","session_id":"s1"}"#,
        );
        assert!(matches!(rs, AgentRunStreamEvent::RunStarted { .. }));
        let td = parse_agent_run_event("", r#"{"type":"text_delta","text":"hi"}"#);
        match td {
            AgentRunStreamEvent::TextDelta { text } => assert_eq!(text, "hi"),
            _ => panic!("expected text_delta"),
        }
        let ltr = parse_agent_run_event(
            "",
            r#"{"type":"local_tool_request","request_id":"q1","name":"shell","input":{"cmd":"ls"}}"#,
        );
        match ltr {
            AgentRunStreamEvent::LocalToolRequest {
                request_id, name, ..
            } => {
                assert_eq!(request_id, "q1");
                assert_eq!(name, "shell");
            }
            _ => panic!("expected local_tool_request"),
        }
        let usage = parse_agent_run_event(
            "",
            r#"{"type":"usage","input_tokens":10,"output_tokens":5,"exact":true}"#,
        );
        match usage {
            AgentRunStreamEvent::Usage { usage } => {
                assert_eq!(usage.input_tokens, Some(10));
                assert_eq!(usage.output_tokens, Some(5));
                assert_eq!(usage.exact, Some(true));
            }
            _ => panic!("expected usage"),
        }
        let done =
            parse_agent_run_event("", r#"{"type":"done","run_id":"r1","status":"completed"}"#);
        match done {
            AgentRunStreamEvent::Done { run_id, status } => {
                assert_eq!(run_id, "r1");
                assert_eq!(status, "completed");
            }
            _ => panic!("expected done"),
        }
    }

    #[test]
    fn agent_run_error_event_parses_payload() {
        let ev = parse_agent_run_event(
            "",
            r#"{"type":"error","error":{"code":"E1","message":"boom","stage":"plan","retryable":true}}"#,
        );
        match ev {
            AgentRunStreamEvent::Error { error } => {
                assert_eq!(error.code.as_deref(), Some("E1"));
                assert_eq!(error.message, "boom");
                assert_eq!(error.stage.as_deref(), Some("plan"));
                assert_eq!(error.retryable, Some(true));
            }
            _ => panic!("expected error"),
        }
    }

    // === 🔴 红线 1b: RemoteControlEvent 未知 type → None 静默丢弃（与上相反）===
    #[test]
    fn remote_control_unknown_event_is_none() {
        let v: Value = serde_json::from_str(r#"{"type":"totally_new_event","foo":1}"#).unwrap();
        assert!(parse_remote_control_event(&v).is_none());
    }

    #[test]
    fn remote_control_missing_field_is_none() {
        // text_delta 缺 index → None。
        let v: Value = serde_json::from_str(r#"{"type":"text_delta","text":"hi"}"#).unwrap();
        assert!(parse_remote_control_event(&v).is_none());
        // 无 type → None。
        let v2: Value = serde_json::from_str(r#"{"foo":1}"#).unwrap();
        assert!(parse_remote_control_event(&v2).is_none());
    }

    // === 红线: 11 事件解析 ===
    #[test]
    fn remote_control_all_eleven_events_parse() {
        let cases: &[(&str, &str)] = &[
            (
                "text_delta",
                r#"{"type":"text_delta","index":0,"text":"a"}"#,
            ),
            (
                "reasoning_delta",
                r#"{"type":"reasoning_delta","index":1,"text":"b"}"#,
            ),
            (
                "tool_call",
                r#"{"type":"tool_call","tool_call_id":"t1","name":"sh"}"#,
            ),
            (
                "tool_result",
                r#"{"type":"tool_result","tool_call_id":"t1","ok":true}"#,
            ),
            (
                "permission_request",
                r#"{"type":"permission_request","request_id":"p1","kind":"shell"}"#,
            ),
            (
                "permission_result",
                r#"{"type":"permission_result","request_id":"p1","decision":"allow"}"#,
            ),
            ("usage", r#"{"type":"usage","input_tokens":3}"#),
            (
                "settle",
                r#"{"type":"settle","status":"settled","billed":true}"#,
            ),
            ("status", r#"{"type":"status","phase":"running"}"#),
            ("error", r#"{"type":"error","code":"E","message":"m"}"#),
            (
                "done",
                r#"{"type":"done","reason":"ok","run_id":"r1","final_status":"completed"}"#,
            ),
        ];
        for (name, payload) in cases {
            let v: Value = serde_json::from_str(payload).unwrap();
            let ev = parse_remote_control_event(&v)
                .unwrap_or_else(|| panic!("event {name} should parse"));
            assert_eq!(ev.type_str(), *name);
        }
    }

    #[test]
    fn remote_control_terminal_only_done_and_settle() {
        let done = RemoteControlEvent::Done {
            reason: "ok".into(),
            run_id: "r".into(),
            final_status: "completed".into(),
        };
        let settle = RemoteControlEvent::Settle {
            status: "settled".into(),
            billed: Some(true),
        };
        let err = RemoteControlEvent::Error {
            code: "E".into(),
            message: "m".into(),
            retryable: None,
            kind: None,
        };
        assert!(is_terminal_remote_event(&done));
        assert!(is_terminal_remote_event(&settle));
        // error 非终结（契约 §4）。
        assert!(!is_terminal_remote_event(&err));
    }

    #[test]
    fn remote_control_camel_snake_aliases() {
        let v: Value =
            serde_json::from_str(r#"{"type":"tool_call","toolCallId":"t9","name":"x"}"#).unwrap();
        match parse_remote_control_event(&v).unwrap() {
            RemoteControlEvent::ToolCall { tool_call_id, .. } => assert_eq!(tool_call_id, "t9"),
            _ => panic!("expected tool_call"),
        }
    }

    // === 红线: 本地工具回调硬超时（忽略 signal 的 handler 也不永挂）===
    #[tokio::test(start_paused = true)]
    async fn local_tool_timeout_when_handler_ignores_signal() {
        let mut handlers: HashMap<String, _> = HashMap::new();
        handlers.insert(
            "slow".to_string(),
            |_input: Value, _ctx: LocalToolContext| async move {
                // 完全忽略取消信号，睡很久。
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                Ok(Value::Null)
            },
        );
        let result =
            invoke_local_tool("run1", "req1", "slow", Value::Null, &handlers, 50, None).await;
        assert!(!result.ok);
        assert_eq!(result.request_id, "req1");
        assert!(result.error.unwrap().contains("timed out"));
    }

    #[tokio::test]
    async fn local_tool_no_handler_rejected() {
        // 含一个无关 handler（同型 map），探测 missing 名应被拒。
        let mut handlers: HashMap<String, _> = HashMap::new();
        handlers.insert(
            "present".to_string(),
            |input: Value, _ctx: LocalToolContext| async move { Ok(input) },
        );
        let result = invoke_local_tool(
            "run1",
            "req1",
            "missing",
            Value::Null,
            &handlers,
            1000,
            None,
        )
        .await;
        assert!(!result.ok);
        assert!(result.error.unwrap().contains("no handler"));
    }

    #[tokio::test]
    async fn local_tool_success_returns_content() {
        let mut handlers: HashMap<String, _> = HashMap::new();
        handlers.insert(
            "echo".to_string(),
            |input: Value, _ctx: LocalToolContext| async move { Ok(input) },
        );
        let result = invoke_local_tool(
            "run1",
            "req1",
            "echo",
            json!({"a":1}),
            &handlers,
            1000,
            None,
        )
        .await;
        assert!(result.ok);
        assert_eq!(result.content, Some(json!({"a":1})));
    }

    // === create_remote_run 校验 ===
    #[test]
    fn to_wire_create_request_remote_fields() {
        let req = AgentRunCreateRequest {
            app_id: "app".into(),
            input: "hi".into(),
            runtime: Some("crabcode_remote".into()),
            runner: Some(crate::agent_runs::RunnerKind::from("cloud")),
            adapter: Some(crate::agent_runs::AdapterKind::from("remote_io")),
            byok_credential_ref: Some("cred-ref-1".into()),
            ..Default::default()
        };
        let wire = to_wire_create_request(&req);
        assert_eq!(wire["app_id"], "app");
        assert_eq!(wire["runtime"], "crabcode_remote");
        assert_eq!(wire["runner"], "cloud");
        assert_eq!(wire["adapter"], "remote_io");
        assert_eq!(wire["byok_credential_ref"], "cred-ref-1");
    }

    #[test]
    fn content_disposition_filename_parsing() {
        assert_eq!(
            filename_from_content_disposition(Some("attachment; filename=\"report.zip\"")),
            Some("report.zip".to_string())
        );
        assert_eq!(
            filename_from_content_disposition(Some("attachment; filename*=UTF-8''a%20b.txt")),
            Some("a b.txt".to_string())
        );
        assert_eq!(filename_from_content_disposition(None), None);
    }
}
