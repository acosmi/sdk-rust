# Changelog

All notable changes to `acosmi-sdk` (Rust) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

版本号对齐 npm 主线 [`@acosmi/sdk-ts`](https://github.com/acosmi/sdk-ts)（事实标准主实现）；
跨语言契约（snake_case wire-format / 符号名对齐 / bug-for-bug 行为）见
[`docs/开发与发布手册.md`](./docs/开发与发布手册.md) §5。

## [2.9.0] - 2026-06-20 — 向量 (Embedding) + 重排序 (Rerank) 端点

托管模型网关新增向量与重排序两类模型（上游接阿里云百炼 DashScope），SDK 订阅会员可经现有会员计费体系（Hold→Settle→Release，按 `total_tokens` 套 input 费率）直接调用。具体上游模型名（`text-embedding-v4` / `gte-rerank-v2` / `qwen3-rerank` 等）由管理员在托管模型后台自填，不在 SDK / 网关硬编码。与 `@acosmi/sdk-ts` v2.9.0 同步。

### Added

- **`Client::embeddings(model_id, &EmbeddingRequest, signal)`** — 向量（同步，`POST /managed-models/:id/embeddings`）：请求 `{ input, dimensions?, encoding_format? }`（`input` = `EmbeddingInput::Single | Batch`），响应为 OpenAI `/v1/embeddings` 标准（`EmbeddingResponse`，网关直通无包装）。仅 `capabilities.supports_embedding=true` 的模型可用。
- **`Client::rerank(model_id, &RerankRequest, signal)`** — 重排序（同步，`POST /managed-models/:id/rerank`）：统一扁平契约 `{ query, documents, top_n?, return_documents?, instruct? }`，响应 `RerankResponse { results: [{ index, relevance_score, document? }], usage, model }`（网关已归一化原生嵌套 / OpenAI 兼容扁平两线路）。仅 `capabilities.supports_rerank=true` 的模型可用。
- **类型** — `EmbeddingInput`/`EmbeddingRequest`/`EmbeddingData`/`EmbeddingUsage`/`EmbeddingResponse`/`RerankRequest`/`RerankResult`/`RerankResponse`（crate 根 re-export）。

## [2.8.0] - 2026-06-19 — Rust SDK 首版（端口自 sdk-ts v2.8.0）

Acosmi Rust SDK 首个发布版本，从事实标准主实现 `@acosmi/sdk-ts` v2.8.0 全量端口。
18 业务域全覆盖，公开符号名（类型 PascalCase / 错误名）与 TS 跨语言一致，
wire-format 字段名 snake_case 与上游 Go json tag 0 偏差。

### Added — 18 业务域

- **core / Client** — `Client::new`（同步）/ `Client::create`（async 预加载 TokenStore）；
  `chat`（同步）/ `chat_stream` / `chat_stream_with_usage`（`impl Stream`）/ `chat_messages` /
  `build_chat_request`；图片视频生成 `generate_image`（同步）/ `generate_video` / `poll_video_task`；
  模型 `list_models` / `list_models_with_status` / `get_model_capabilities` / `get_quota_summary` /
  `ensure_model_cached`；`server_url` / `compliance_base_url` / `api_base_url` 三根地址契约 +
  `normalize_gateway_base_url`（仅 http/https，拒 ws/wss）。
- **models（双 adapter 红线）** — `Adapter::{Anthropic, OpenAI}` 等地位，`get_adapter_for_model`
  按 `preferred_format` / `supported_formats` 选路（含 v2.5.1 格式一致性护栏）；`OpenAIStreamConverter`
  OpenAI→Anthropic 流式归一；`ManagedModel` catalog + 档位门控字段（`locked` / `free_tier` /
  `min_plan_tier` / `chat_runtime_supported` / `default_tool_ids`）；输入模态 helper
  （`model_supports_input_modality` / `model_supports_image_input` /
  `find_desktop_visual_understanding_model`）；`new_web_search_tool`（Anthropic Web Search Tool）；
  `validate_end_user_id`（用户隔离，跨 provider 通用语义）。
- **auth** — OAuth 2.1 PKCE 原语 `discover` / `register` / `exchange_code` / `refresh_token` /
  `revoke_token` / `generate_code_verifier` / `code_challenge`（S256）/ `generate_state` /
  `new_token_set`；Web OAuth 原语 `discover_web_oauth_metadata` / `register_web_oauth_client` /
  `create_web_authorization_request` / `complete_web_authorization_request`；scope helper
  `all_scopes` / `model_scopes` / `compliance_scopes` / `chat_bridge_scopes` /
  `remote_control_scopes`（远控/桥接为高风险 scope，不进 `all_scopes()`）。
- **agent_runs** — `Client::agent_runs()`：`create` / `stream`（durable replay）/ `run` / `cancel` /
  `get` / `list_artifacts` / `download_artifact` / `submit_local_tool_result`（本地只读工具桥协议）；
  远程控制 `create_remote_run` / `stream_remote_control` + 11 事件 `RemoteControlEvent` union +
  `parse_remote_control_event` / `is_terminal_remote_event`；远控管理面 `list` /
  `submit_permission_result` / `submit_user_message` / `reveal_remote_token`；BYOK
  `CrabCodeByokClient`（`list` / `create` / `rotate` / `revoke`，明文一次性提交、masked 视图）。
- **chatbridge** — `Client::chat_bridge()`：集成/凭证管理面 CRUD（`create_integration` /
  `list_integrations` / `store_credential` / `rotate_credential` / `revoke_credential` …）；
  `ChatCredentialPublic` 编译期即无密文字段（secret 零导出红线）；类型守卫 `is_platform` /
  `is_region` / `as_credential_ref`（branded `CredentialRef` 防 plaintext 误传）。
- **compliance（最大子树）** — `Client::compliance()`：电子证据（`create_evidence_asset` /
  `verify_evidence_public` 匿名公开验真 / `build_evidence_package`）/ 时间章（`issue_timestamp` /
  `verify_timestamp` / `wait_for_timestamp_verified` / `list_tsa_providers`）/ 出证报告
  （`create_report` / `publish_report` step-up / `download_report`）/ 签署 envelope
  （`create_signing_envelope` / `sign_envelope` / `create_h5_signing_url` / `void_envelope`）/
  用印审批（`submit_seal_approval` / `approve_seal_approval` step-up / `list_seal_uses`）/
  合同模板全生命周期（DRAFT→PUBLISHED→ARCHIVED）/ 能力闸门与操作投影（`get_capabilities`
  fail-closed / `list_operations`）；错误分类 `classify_compliance_error`（Java 数值码 → symbolic key）；
  写操作幂等（`Idempotency-Key`）+ 401 不重放 + 禁自动重试。**SDK 永不接触 provider endpoint /
  证书密钥 / raw payload / callback billing**。
- **billing** — 钱包（`get_wallet_stats` / `get_wallet_transactions`，金额 f64）/ 余额权益
  （`get_balance` / `get_balance_detail` / `list_entitlements` / `claim_monthly_free` / `get_by_model`）/
  流量包（`list_token_packages` / `buy_token_package` / `wait_for_payment`，`PaymentMethod` 枚举）/
  消费记录分页。
- **skills** — 技能商店浏览（`browse_skill_store` / `browse_skills`）/ 安装下载 / 生成优化
  （`generate_skill` / `optimize_skill`）/ 认证（`certify_skill` / `get_certification_status`）/ 工具
  （`list_tools` / `get_tool`）。
- **notifications** — 通知列表 / 已读 / 偏好 / 设备 + WebSocket 实时推送（`tokio-tungstenite`，
  一次性 stream-ticket 鉴权，对齐 Go gorilla/websocket）。
- **subscription** — `get_membership` / `get_subscription_tier` / `subscription_precheck` /
  `list_plans` / `get_plan_by_code`。
- **pricing / products** — 公开定价配置 / 合规报价 / 商品中心索引（productFamily / audience /
  billingMode）/ 公开模型列表。
- **casehall** — 律师库（VERIFIED + ACTIVE，PII L3 脱敏）/ 案件线索 / 咨询 / 法律服务 SKU /
  律师资质自查。
- **enterprise** — 企业席位 / 成员邀请 / 组织订阅 / 用量报表 / 企业 OWNER KYC 自查。
- **finance** — 发票 / 退款 / 对公转账；所有 `*_fen` 金额为 `i64` 整数分（2^53 内无浮点风险）。
- **support** — `submit_bug_report` / `get_bug_report`（公开页 ViewModel）。
- **shared** — 跨域 `Error`（`thiserror` 顶层 enum，`match` 取代 TS 7 个 `instanceof` class）/
  `Result` / 分页（`PageResult<T>`）/ 幂等键 / `RetryAdvice` 叠加层。
- **sanitize**（feature，默认开） — 历史消息白名单过滤 + 深度/尺寸校验 + ephemeral 剥离，
  经 `core::sanitize_bridge` 与 `Client` 接通；默认零开销。

### 设计红线（bug-for-bug 对齐 TS / Go 主实现）

- **双格式等地位**：`Adapter::Anthropic` + `Adapter::OpenAI` 恒编译、不可降级、不 feature-gate。
- **流式路径永不重试**（防双扣）；**POST 默认不重试**（计费安全）；**401 单次重试防递归**。
- **sanitize 对 `thinking` / `redacted_thinking` 块硬豁免**（Anthropic 续轮原样回传约束）。
- **金额三阵营**：钱包 `f64` / finance·商品化 `*_fen`=`i64` / `json.Number` 类十进制=`String`。
- **secret 零导出**：chatbridge / BYOK 凭证编译期即无密文字段，只回 masked 视图。

### 实现要点（Rust 适配）

- `#![forbid(unsafe_code)]`；运行时仅 `tokio` + `reqwest`（rustls TLS）。
- 流式走 `impl Stream`（`async-stream` + `futures`），取消走 `tokio_util::CancellationToken`
  （取代 TS `AbortSignal`）。
- 方法名 snake_case（Rust 惯例），类型名 / 错误名 PascalCase 跨语言保留；`Option<T>` 取代
  TS optional；`enum Error` + `match` 取代 7 个错误 class + `instanceof`。
- 测试：157 通过（137 lib + 10 P1 + 4 P2 + 6 P5），覆盖双 adapter 选路 / 流式不重试 /
  POST 不重试 / 401 防递归 / sanitize thinking 豁免 / `OpenAIStreamConverter` / `ensure_token`
  单航班续期等 P0 红线。

[2.8.0]: https://github.com/acosmi/sdk-rust/releases/tag/v2.8.0
</content>
