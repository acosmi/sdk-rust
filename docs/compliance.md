# Compliance SDK 指南（Rust）

本指南覆盖 `acosmi-sdk` 合规域（compliance）的公开 Rust 接口：时间章、证据资产、
证据包、出证报告、签署 envelope、用印审批、合同模板、provider 请求轮询。

> 端口自 `acosmi-sdk-ts/docs/compliance.md`（TS v2.8.0）。本文档随源码演进——所有方法名、
> scope 常量、类型字段均与 `src/compliance/` 严格对齐；代码块镜像已被 CI `cargo build --examples`
> 编译验证的 `examples/compliance_*.rs`。

SDK 只暴露 Acosmi 领域对象。它**不暴露** provider 凭据、provider endpoint、provider 原始报文、
证书、私钥、签名容器、扣费 commit 内部细节。完整禁带清单见文末 *安全边界*。

## 目录

- [快速开始](#快速开始)
- [Scopes](#scopes)
- [Base URL](#base-url)
- [幂等与重试规则](#幂等与重试规则)
- [证据 / 时间章 / 报告链路](#证据--时间章--报告链路)
- [公开验证](#公开验证)
- [签署与 provider 请求轮询](#签署与-provider-请求轮询)
- [分页列表](#分页列表)
- [能力闸门与操作投影](#能力闸门与操作投影)
- [TSA 只读视图](#tsa-只读视图)
- [Envelope 收尾](#envelope-收尾)
- [合同模板](#合同模板)
- [错误分类](#错误分类)
- [方法成熟度](#方法成熟度)
- [安全边界](#安全边界)
- [打包示例](#打包示例)

## 快速开始

合规域走独立子客户端 [`ComplianceClient`]，经 `Client::compliance()` getter 获取（构造廉价，
可随用随取）。下面是 hash-only 证据 + 时间章 + 出证报告的完整链路（镜像
`examples/compliance_timestamp.rs`）：

```rust,no_run
use std::collections::HashMap;

use acosmi::compliance::{ComplianceAssetType, ComplianceDigestSource, CompliancePrivacyLevel};
use acosmi::{
    Client, CompliancePollOptions, ComplianceWriteOptions, Config, CreateEvidenceAssetRequest,
    CreateReportRequest,
};
use sha2::{Digest, Sha256};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::create(Config {
    server_url: Some(std::env::var("ACOSMI_SERVER_URL")?),
    // 缺省 ${server_url}/admin-api；仅当 compliance 走独立 ingress 时才显式设置。
    compliance_base_url: std::env::var("ACOSMI_COMPLIANCE_BASE_URL").ok(),
    ..Default::default()
})
.await?;

// 按业务最小集合申请 compliance scope（不要一次性申请全部）。
let scopes: Vec<String> = vec![
    "compliance:evidence:read".into(),
    "compliance:evidence:write".into(),
    "compliance:timestamp:issue".into(),
    "compliance:timestamp:verify".into(),
    "compliance:reports:read".into(),
    "compliance:reports:write".into(),
];
client.login("Compliance Example", &scopes, None).await?;

let cc = client.compliance();

// 1) 本地对业务内容做 sha256（只传哈希，不传原文）。
let sha256_hex = hex::encode(Sha256::digest(b"release v1.2.3 manifest"));

// 2) 创建 hash-only 证据资产。idempotency-key 必须在进程外持久化。
let asset = cc
    .create_evidence_asset(
        &CreateEvidenceAssetRequest {
            asset_type: ComplianceAssetType::from(ComplianceAssetType::HASH_ONLY),
            name: "release-manifest".into(),
            hash_algorithm: acosmi::compliance::ComplianceHashAlgorithm::from(
                acosmi::compliance::ComplianceHashAlgorithm::SHA256,
            ),
            declared_hash: Some(sha256_hex),
            digest_source: Some(ComplianceDigestSource::from(ComplianceDigestSource::CLIENT)),
            privacy_level: Some(CompliancePrivacyLevel::from(CompliancePrivacyLevel::PRIVATE)),
            ..Default::default()
        },
        &ComplianceWriteOptions { idempotency_key: Some("asset:order-123".into()) },
        None,
    )
    .await?;

// 3) 给资产申请时间章。
let token = cc
    .issue_timestamp_for_asset(
        asset.id,
        &ComplianceWriteOptions { idempotency_key: Some(format!("ts:{}", asset.evidence_no)) },
        None,
    )
    .await?;

// 4) polling 到本地 verify 通过。
let verified = cc
    .wait_for_timestamp_verified(token.id, &CompliancePollOptions::default(), None)
    .await?;
println!("{}", verified.verification_status);
# Ok(()) }
```

> 上例用了 `hex` / `sha2` crate 演示本地摘要——它们不是 `acosmi-sdk` 的依赖，调用方按需自取。

## Scopes

按最小够用原则申请 scope。[`compliance_scopes()`] 返回全部合规 scope，但生产应用通常只取子集。
scope 常量定义在 [`acosmi::compliance`] 下（`src/compliance/scopes.rs`）：

```rust
use acosmi::compliance::{
    SCOPE_COMPLIANCE_EVIDENCE_READ, SCOPE_COMPLIANCE_REPORTS_READ,
    SCOPE_COMPLIANCE_TIMESTAMP_VERIFY,
};

# fn pick() -> Vec<String> {
let scopes: Vec<String> = vec![
    SCOPE_COMPLIANCE_EVIDENCE_READ.into(),
    SCOPE_COMPLIANCE_TIMESTAMP_VERIFY.into(),
    SCOPE_COMPLIANCE_REPORTS_READ.into(),
];
# scopes }
```

合规 scope 与 `ScopeAI` / `ScopeSkills` / `ScopeAccount` **相互独立**——持有通用 scope 不授予任何合规权限。
服务端按细粒度逐项校验，SDK 也只应按需申请最小集合。

完整 scope 常量（字面量与 Go `DesktopOAuthScopes` / Java `ComplianceScopes` 三处必须一致）：

| 常量 | 字面量 | 用途 |
| --- | --- | --- |
| `SCOPE_COMPLIANCE_EVIDENCE_READ` | `compliance:evidence:read` | 读证据资产 / 包 |
| `SCOPE_COMPLIANCE_EVIDENCE_WRITE` | `compliance:evidence:write` | 创建证据资产 / 构建包 |
| `SCOPE_COMPLIANCE_TIMESTAMP_ISSUE` | `compliance:timestamp:issue` | 申请时间章 |
| `SCOPE_COMPLIANCE_TIMESTAMP_VERIFY` | `compliance:timestamp:verify` | 验证时间章 |
| `SCOPE_COMPLIANCE_CONTRACT_SIGNING_READ` | `compliance:contract_signing:read` | 读 envelope / 合同 / 用印执行 |
| `SCOPE_COMPLIANCE_CONTRACT_SIGNING_WRITE` | `compliance:contract_signing:write` | 创建 / 签署 envelope |
| `SCOPE_COMPLIANCE_SEAL_MANAGE` | `compliance:seal:manage` | 印章管理（后端延后） |
| `SCOPE_COMPLIANCE_SEAL_APPROVAL_REQUEST` | `compliance:seal_approval:request` | 提交 / 撤销用印审批 |
| `SCOPE_COMPLIANCE_SEAL_APPROVAL_APPROVE` | `compliance:seal_approval:approve` | 审批 / 驳回（gated，需 step-up） |
| `SCOPE_COMPLIANCE_SEAL_USE_EXECUTE` | `compliance:seal_use:execute` | 执行用印（后端延后） |
| `SCOPE_COMPLIANCE_REPORTS_READ` | `compliance:reports:read` | `get_report` / `download_report` |
| `SCOPE_COMPLIANCE_REPORTS_WRITE` | `compliance:reports:write` | `create_report` |
| `SCOPE_COMPLIANCE_REPORTS_PUBLISH` | `compliance:reports:publish` | `publish_report`（gated，需 step-up） |
| `SCOPE_COMPLIANCE_CONTRACT_TEMPLATE_READ` | `compliance:contract_template:read` | 读合同模板 / 版本 |
| `SCOPE_COMPLIANCE_CONTRACT_TEMPLATE_WRITE` | `compliance:contract_template:write` | 合同模板写（创建 / 改 / 删 / 上传 / 发布 / 归档） |

报告 scope 按动作拆分：`*_READ`（`get_report` / `download_report`）、`*_WRITE`（`create_report`）、
`*_PUBLISH`（`publish_report`，另需 step-up）。`*_WRITE` 与 `*_PUBLISH` 是后加的——更早签发的
token **不携带**它们；调用 `create_report` / `publish_report` 的应用必须在 `login()` 时申请对应 scope，
老用户需重新授权（再走一遍 OAuth）才能拿到新 scope。

合同模板 scope（`*_CONTRACT_TEMPLATE_READ` / `*_WRITE`）按读写方向拆分、**不需要** step-up；
同样是后加 scope，老 token 不携带，需重新授权。

## Base URL

`Client` 把既有模型网关路径保持在 `/api/v4` 下。合规域用 `client.compliance_url(path)`，缺省
`${server_url}/admin-api`，因此**不与既有 API 路径冲突**。只有当 compliance 通过独立 ingress 暴露时，
才设置 `Config.compliance_base_url`：

```rust,no_run
use acosmi::{Client, Config};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::create(Config {
    server_url: Some(std::env::var("ACOSMI_SERVER_URL")?),
    compliance_base_url: Some(std::env::var("ACOSMI_COMPLIANCE_BASE_URL")?),
    ..Default::default()
})
.await?;
# let _ = client; Ok(()) }
```

> `compliance_base_url` 是独立的第二根地址，**不被** `server_url` 派生覆盖。两者各自归一化。

## 幂等与重试规则

每个合规**写**方法都接 [`ComplianceWriteOptions`]（携带 `idempotency_key`）和一个独立的
`signal: Option<CancellationToken>`：

```rust,no_run
# use acosmi::{ComplianceWriteOptions, compliance::ComplianceClient};
# async fn demo(cc: &ComplianceClient, report_id: i64, signal: tokio_util::sync::CancellationToken) -> Result<(), Box<dyn std::error::Error>> {
cc.publish_report(
    report_id,
    &ComplianceWriteOptions { idempotency_key: Some("publish:report-77".into()) },
    Some(signal),
)
.await?;
# Ok(()) }
```

> Rust 把 TS `ComplianceWriteOptions { idempotencyKey, signal }` 中的 `signal` 拆为独立末位参数，
> 与 SDK 其余异步方法的取消约定一致。`ComplianceWriteOptions` 只剩 `idempotency_key` 一个字段。

发请求**前**就把 idempotency-key 持久化到进程外（DB / 本地文件 / 业务订单表）。重启、超时、网络失败、
401 之后，**对同一业务动作复用同一个 key**。

合规写方法**故意不做自动重试**——这条红线与普通 API 不同：

- POST / PUT / DELETE 写调用**不**重试 5xx、429、超时、传输错误。
- 写调用**不**在 401 时 refresh + replay。401 直接抛 [`Error::Http`]，需重新登录后用**同一 key** 重调。
- GET 读调用可做**一次**安全的 401 refresh 重试。
- 调用方拥有用户重认证的所有权，恢复同一业务动作时必须复用同一 idempotency-key。

## 证据 / 时间章 / 报告链路

```rust,no_run
# use acosmi::{ComplianceWriteOptions, CompliancePollOptions, CreateEvidenceAssetRequest, CreateReportRequest, compliance::ComplianceClient};
# async fn demo(cc: &ComplianceClient, asset_req: CreateEvidenceAssetRequest) -> Result<(), Box<dyn std::error::Error>> {
let asset = cc
    .create_evidence_asset(&asset_req, &ComplianceWriteOptions { idempotency_key: Some("asset-key".into()) }, None)
    .await?;

let token = cc
    .issue_timestamp_for_asset(asset.id, &ComplianceWriteOptions { idempotency_key: Some("ts-key".into()) }, None)
    .await?;

cc.wait_for_timestamp_verified(token.id, &CompliancePollOptions::default(), None).await?;

// build_evidence_package(asset_id, timestamp_token_id?, opts, signal)
let pkg = cc
    .build_evidence_package(asset.id, Some(token.id), &ComplianceWriteOptions { idempotency_key: Some("pkg-key".into()) }, None)
    .await?;

let report = cc
    .create_report(
        &CreateReportRequest { asset_id: asset.id, package_id: pkg.id },
        &ComplianceWriteOptions { idempotency_key: Some("report-key".into()) },
        None,
    )
    .await?;

let download = cc.download_report(report.id, None).await?;
println!("{} {:?}", download.report_no, download.timestamp_serial_number);
# Ok(()) }
```

`download_report` 返回**离线复核视图**：报告哈希、资产哈希、包哈希、时间章摘要。它**不**包含
合同正文、storage key、provider 原始报文、主体快照。

时间章轮询用 [`CompliancePollOptions`] 控制超时与退避（`timeout_ms` / `initial_interval_ms` /
`max_interval_ms`，全部 `Option`，缺省由 SDK 选）。polling 失败时返回 [`CompliancePollError`]，其
`kind` 字段是 [`CompliancePollErrorKind`]：`Timeout`（仍 UNKNOWN，**勿**自动重发 provider 请求）、
`TerminalFailure`（本地 verify 失败，**勿**用同一 key 重发）、`StepUpRequired`、`Unknown`。

## 公开验证

`verify_evidence_public` 返回**隐私保护**的验证结果：

```rust,no_run
# use acosmi::compliance::ComplianceClient;
# async fn demo(cc: &ComplianceClient) -> Result<(), Box<dyn std::error::Error>> {
// 第 1 参 evidence_no、第 2 参 storage_ref（二选一）、第 3 参 signal。
let result = cc.verify_evidence_public(Some("EV-2026-0001"), None, None).await?;
println!("content_hash={} verified_at={}", result.content_hash, result.verified_at);
# Ok(()) }
```

公开结果只含稳定的证据与哈希字段。它**排除** PII、合同原文、storage bucket/key、主体快照 id、
provider 原始报文、TSA 内部字段。

`verify_evidence_public` **可匿名调用**：不要求先 `login()`。无 token 时 SDK 发匿名请求而非抛
`not authorized, call login() first`；已持 token 时请求带 `Authorization` 头以便后端保留审计上下文。
与认证 GET 读不同，公开验证**永不**在 401 时触发 refresh/replay。

## 签署与 provider 请求轮询

签署 envelope 方法只暴露 Acosmi 工作流状态，不暴露 provider 专有字段。`sign_envelope` 和
`create_h5_signing_url` 可能返回 step-up 或 gate-closed 业务错误；调用方应**呈现**这些状态而非重试：

```rust,no_run
use acosmi::compliance::ComplianceErrorKey;
use acosmi::{classify_compliance_error, is_compliance_business_error, ComplianceWriteOptions, SignEnvelopeRequest};
# use acosmi::compliance::ComplianceClient;

# async fn demo(cc: &ComplianceClient, envelope_id: i64, req: SignEnvelopeRequest) -> Result<(), Box<dyn std::error::Error>> {
let result = cc
    .sign_envelope(envelope_id, &req, &ComplianceWriteOptions { idempotency_key: Some("sign-key".into()) }, None)
    .await;

if let Err(e) = result {
    if is_compliance_business_error(&e) {
        let info = classify_compliance_error(&e);
        if info.step_up_required {
            // 引导用户重做 OAuth introspection / 升级 token，再用同一 key 重试。
            return Ok(());
        }
        if info.key == ComplianceErrorKey::EnvelopeGateClosed {
            // 后端闸门未开放——勿重试，向用户展示「功能开放中」。
            return Ok(());
        }
        if info.terminal {
            // 终态——重试无用。
            return Ok(());
        }
    }
    return Err(e.into());
}
# Ok(()) }
```

provider 请求轮询是只读的，暴露公开状态视图：

```rust,no_run
# use acosmi::{CompliancePollOptions, compliance::ComplianceClient};
# async fn demo(cc: &ComplianceClient, provider_request_id: i64) -> Result<(), Box<dyn std::error::Error>> {
let view = cc
    .wait_for_provider_request_terminal(
        provider_request_id,
        &CompliancePollOptions { timeout_ms: Some(30_000), ..Default::default() },
        None,
    )
    .await?;

if view.status.as_str() == "SUCCESS" {
    // provider SUCCESS 不等于扣费 commit；以 envelope / report 业务状态为准。
}
# Ok(()) }
```

## 分页列表

SDK 暴露针对后端合规网关的分页读（`GET .../page`）。每个返回 yudao [`PageResult<T>`]
（`{ total, list }`——全 SDK 唯一分页结果形态）：

```rust,no_run
# use acosmi::{compliance::{ComplianceClient, ListSealApprovalsRequest}, shared::pagination::PageRequest};
# async fn demo(cc: &ComplianceClient) -> Result<(), Box<dyn std::error::Error>> {
let page = cc
    .list_seal_approvals(
        &ListSealApprovalsRequest {
            page: PageRequest { page_no: Some(1), page_size: Some(20), ..Default::default() },
            status: Some("PENDING".into()),
            create_time_start: Some("2026-05-01 00:00:00".into()),
            create_time_end: Some("2026-05-22 23:59:59".into()),
            ..Default::default()
        },
        None,
    )
    .await?;
println!("{} / {}", page.total, page.list.len());
# Ok(()) }
```

请求参数都内嵌共享的 [`PageRequest`]（`page_no` / `page_size` / `sort_by` / `sort_direction`，
全部 `Option`；省略则后端选缺省）加每方法专属过滤字段：

| 方法 | Endpoint | 过滤字段（全部可选） |
| --- | --- | --- |
| `list_evidence_assets` | `GET /compliance/evidence/assets/page` | `asset_type` `status` `create_time_start` `create_time_end` |
| `list_timestamps` | `GET /compliance/timestamps/page` | `provider` `verification_status` `create_time_start` `create_time_end` |
| `list_evidence_packages` | `GET /compliance/evidence/packages/page` | `status` `create_time_start` `create_time_end` |
| `list_reports` | `GET /compliance/reports/page` | `status` `create_time_start` `create_time_end` |
| `list_signing_envelopes` | `GET /compliance/signing-envelopes/page` | `status` `create_time_start` `create_time_end` |
| `list_seal_approvals` | `GET /compliance/seal-approvals/page` | `status` `create_time_start` `create_time_end` |
| `list_seal_uses` | `GET /compliance/seal-uses/page` | `seal_id` `envelope_id` `usage_status` `create_time_start` `create_time_end` |

`create_time_start` / `create_time_end` 是调用方提供的日期时间**字符串**。后端按
`yyyy-MM-dd HH:mm:ss` 解析（例如 `"2026-05-01 00:00:00"`）。SDK 原样透传——**不**校验格式、**不**转时区。

这些是认证 GET 读，遵循与 `get_evidence_asset` / `get_report` 相同的读语义：一次安全的 401 refresh-and-replay。

`list_seal_approvals` 与 `list_pending_seal_approvals` 不同——后者只返回 `PENDING` 审批的**纯数组**；
前者是分页的，支持 status / 时间过滤。

各 `*PageItem` 类型（`EvidenceAssetPageItem` / `TimestampPageItem` / `EvidencePackagePageItem` /
`ReportPageItem` / `SigningEnvelopePageItem` / `SealApprovalPageItem` / `SealUsePageItem`）是对应详情视图的
**SDK-safe 子集**加 `create_time`（ISO-8601）字段。它们**永不**暴露 provider 原始报文、证书、storage key、
合同原文。

`list_seal_uses`（合规网关 S6）每行对应一次**用印执行**（envelope / 合同 / 印章 / 审批联动后真正触发的
provider 侧盖章）。它与 envelope 域状态、用印审批工作流**正交**：

- `list_signing_envelopes` → 高层 envelope 域状态。
- `list_seal_approvals` → envelope 上的审批工作流（`PENDING` → `APPROVED` / `REJECTED` / `CANCELED`）。
- `list_seal_uses` → 真正的盖章执行（`invoked_at` → `consumed_at`，终态失败带 `failure_reason`）。

`SealUsePageItem` 字段：`id` `envelope_id` `contract_id` `seal_id`（i64）、`usage_status`、
`sign_location_type`（Option）、`invoked_at` / `consumed_at` / `failure_reason`（Option）、`create_time`。

`list_seal_uses` 复用既有读 scope `SCOPE_COMPLIANCE_CONTRACT_SIGNING_READ`，不引入新 scope。
更广的用印授权层与完整 seal CRUD（gap-register U-3 / U-11）仍在后端延后（CFCA 私有 jar + W3 闸门后），
本版**不**暴露为 SDK 方法。

## 能力闸门与操作投影

SDK 暴露合规网关 S2 读：一个能力闸门查询和一个操作投影视图。

```rust,no_run
# use acosmi::compliance::ComplianceClient;
# async fn demo(cc: &ComplianceClient) -> Result<(), Box<dyn std::error::Error>> {
let caps = cc.get_capabilities(None).await?;
for cap in &caps {
    println!("{} executable={} state={:?}", cap.action, cap.executable, cap.state);
}
# Ok(()) }
```

四个方法（`get_capabilities` / `get_feature_gate` / `list_operations` / `get_operation`）都是认证 GET 读，
遵循 `get_report` / `get_evidence_asset` 相同读语义（一次安全 401 refresh-and-replay）。

### Capabilities

`get_capabilities` 每个高风险 / 计费动作返回一条 [`ComplianceCapability`]：`signEnvelope`、
`createH5SigningUrl`、`publishReport`、`approveSealApproval`、`executeSealUse`、`createSeal`。

`ComplianceCapability` 字段：`action`（String）、`executable`（bool）、`state`
（`FeatureGateState` 开放联合：`executable` / `scope_missing` / `not_provisioned` /
`quota_exceeded` / `step_up_required` / `gate_closed` / `unknown`）、`required_scopes`（`Vec<String>`）、
`required_step_up`（bool）、`reason`（String）。

在调用高风险动作**前**查询能力并据此 gate UI。**取不到能力时 fail-closed**——把动作当作
`executable: false`。

`get_feature_gate(action, signal)` 是便捷方法：它内部 fetch `get_capabilities` 并返回 `action` 匹配的那条
（无匹配返回 `None`）。**每次调用发一次网络请求**。要 gate 多个动作时，调一次 `get_capabilities` 然后本地查表，
不要反复调 `get_feature_gate`。

```rust,no_run
# use acosmi::compliance::ComplianceClient;
# async fn demo(cc: &ComplianceClient) -> Result<(), Box<dyn std::error::Error>> {
match cc.get_feature_gate("publishReport", None).await? {
    Some(gate) if gate.executable => { /* 放行 */ }
    Some(gate) if gate.state.as_str() == "step_up_required" => { /* 引导重认证 */ }
    _ => { /* fail-closed */ }
}
# Ok(()) }
```

### 操作投影

操作投影描述单个操作的进度——它与履约对象的域状态**正交**。

```rust,no_run
# use acosmi::{compliance::{ComplianceClient, ListOperationsRequest}, shared::pagination::PageRequest};
# async fn demo(cc: &ComplianceClient) -> Result<(), Box<dyn std::error::Error>> {
let page = cc
    .list_operations(
        &ListOperationsRequest {
            page: PageRequest { page_no: Some(1), page_size: Some(20), ..Default::default() },
            status: Some("failed".into()),
            ..Default::default()
        },
        None,
    )
    .await?;
for op in &page.list {
    println!("{} {} terminal={} retryable={}", op.base.operation_id, op.base.status, op.base.terminal, op.base.retryable);
}

let detail = cc.get_operation(page.list[0].base.id, None).await?;
# let _ = detail; Ok(()) }
```

`list_operations`（`GET /compliance/operations/page`）返回 `PageResult<OperationPageItem>`。
`get_operation`（`GET /compliance/operations/{id}`）取**数值行 id**（不是 `operation_id` 幂等键）。

`OperationPageItem` / `OperationDetail` 把共享字段收在 `base: OperationBase` 里：`id`（i64）、
`operation_id`（String，幂等键）、`status`、`terminal`、`retryable`、`attempt_count`、`business_no`、
`contract_no`、`seal_id`、`reconciliation_status`、`next_retry_at`、`requested_at`、`responded_at`、
`create_time`。时间字段是 ISO-8601。这些视图**永不**暴露 provider 原始报文、证书、storage key、合同原文。

## TSA 只读视图

SDK 暴露合规网关 S3 读：两个时间章颁发机构（TSA）只读视图。

```rust,no_run
# use acosmi::compliance::ComplianceClient;
# async fn demo(cc: &ComplianceClient) -> Result<(), Box<dyn std::error::Error>> {
let providers = cc.list_tsa_providers(None).await?;
for p in &providers {
    println!("{} {} available={}", p.name, p.environment, p.available);
}

let stats = cc.get_tsa_stats(None).await?;
println!("total={}", stats.total);
println!("VERIFIED={}", stats.by_verification_status.get("VERIFIED").copied().unwrap_or(0));
# Ok(()) }
```

`list_tsa_providers`（`GET /compliance/timestamps/providers`）每个已配置 TSA provider 返回一条
[`TsaProvider`]：`name`（String）、`environment`（String，例如 `production` / `sandbox`）、`available`（bool）。
它是只读视图——**永不**暴露 provider endpoint、凭据、证书或其他内部集成材料。

`get_tsa_stats`（`GET /compliance/timestamps/stats`）返回只读聚合：时间章总数 + 按验证状态计数 map。
[`TsaStats`] 字段：`total`（i64）、`by_verification_status`（`HashMap<String, i64>`，键是验证状态枚举名
如 `VERIFIED` / `PENDING` / `FAILED`）。无时间章时 map 可能为空。

两者都是认证 GET 读，遵循 `get_report` / `get_capabilities` 相同读语义。

## Envelope 收尾

SDK 暴露合规网关 S4 的 envelope 收尾面：两个只读视图 + 一个写。

```rust,no_run
# use acosmi::{ComplianceWriteOptions, VoidEnvelopeRequest, compliance::ComplianceClient};
# async fn demo(cc: &ComplianceClient, envelope_id: i64) -> Result<(), Box<dyn std::error::Error>> {
let contracts = cc.list_envelope_contracts(envelope_id, None).await?;
for c in &contracts {
    println!("{} {} {} {}", c.contract_no, c.title, c.status, c.content_hash);
}

let provider_requests = cc.list_envelope_provider_requests(envelope_id, None).await?;
for op in &provider_requests {
    println!("{} {} terminal={}", op.base.operation_id, op.base.status, op.base.terminal);
}

// void_envelope 是写：接 Idempotency-Key，不自动重试，不在 401 refresh/replay。reason 必填。
let voided = cc
    .void_envelope(
        envelope_id,
        &VoidEnvelopeRequest { reason: "signed in error".into() },
        &ComplianceWriteOptions { idempotency_key: Some("void-key".into()) },
        None,
    )
    .await?;
# let _ = voided; Ok(()) }
```

`list_envelope_contracts`（`GET /compliance/signing-envelopes/{id}/contracts`）和
`list_envelope_provider_requests`（`GET .../provider-requests`）是认证 GET 读，返回**纯数组**（非 `PageResult`）。

`EnvelopeContractItem` 字段：`id`、`envelope_id`、`contract_no`、`title`、`mime_type`、`size`、
`hash_algorithm`、`content_hash`、`signed_content_hash`（Option）、`status`、`create_time`。它是 SDK-safe
视图——**永不**暴露合同原文、storage key、provider 原始报文。

`list_envelope_provider_requests` **复用**操作投影类型 `OperationPageItem`（见上节），描述每个 provider 请求
的进度，与 envelope 域状态正交。

`void_envelope`（`POST /compliance/signing-envelopes/{id}/void`）是**写**，遵循合规写规则。
`VoidEnvelopeRequest` 是 `{ reason: String }`。在调用方侧持久化 idempotency-key，恢复同一作废动作时复用。

envelope 收尾的其余动作——send / remind / authorize / download / token——后端延后，本版**不**暴露为 SDK 方法。

## 合同模板

SDK 暴露合规网关 S5 合同模板面：`DRAFT` → `PUBLISHED` → `ARCHIVED` 生命周期，含 PDF 上传、
字段覆盖层、不可变版本快照。

```rust,no_run
use acosmi::compliance::{ContractTemplateField, ContractTemplateFieldType};
use acosmi::{
    ComplianceWriteOptions, CreateContractTemplateRequest, UpdateContractTemplateRequest,
    UploadContractTemplatePdfRequest,
};
# use acosmi::compliance::ComplianceClient;

# async fn demo(cc: &ComplianceClient, pdf_base64: String) -> Result<(), Box<dyn std::error::Error>> {
// 1) 创建（DRAFT）。
let tpl = cc
    .create_contract_template(
        &CreateContractTemplateRequest {
            name: "Mutual NDA".into(),
            description: Some("standard NDA".into()),
            fields: None,
        },
        &ComplianceWriteOptions { idempotency_key: Some("create-key".into()) },
        None,
    )
    .await?;

// 2) 上传 PDF 原文（base64）。pdf_hash / pdf_page_count 随返回的 ContractTemplateResp 回来。
cc.upload_contract_template_pdf(
    tpl.id,
    &UploadContractTemplatePdfRequest { pdf_base64 },
    &ComplianceWriteOptions { idempotency_key: Some("upload-key".into()) },
    None,
)
.await?;

// 3) 编辑字段覆盖层（仅 DRAFT 允许）。
cc.update_contract_template(
    tpl.id,
    &UpdateContractTemplateRequest {
        fields: Some(vec![ContractTemplateField {
            key: "sig-partyA".into(),
            r#type: ContractTemplateFieldType::from(ContractTemplateFieldType::SIGNATURE),
            label: "Party A signature".into(),
            page: 1,
            x: 100.0,
            y: 200.0,
            width: 80.0,
            height: 30.0,
            assigned_role: Some("partyA".into()),
            order: 0,
            required: true,
        }]),
        ..Default::default()
    },
    &ComplianceWriteOptions { idempotency_key: Some("update-key".into()) },
    None,
)
.await?;

// 4) 发布——DRAFT → PUBLISHED；current_version 自增，fields + pdf_hash 冻结进版本表。
cc.publish_contract_template(tpl.id, &ComplianceWriteOptions { idempotency_key: Some("publish-key".into()) }, None).await?;

// 5) 可选归档——PUBLISHED → ARCHIVED；归档后只读。
cc.archive_contract_template(tpl.id, &ComplianceWriteOptions { idempotency_key: Some("archive-key".into()) }, None).await?;
# Ok(()) }
```

9 个方法里：`create_contract_template` / `update_contract_template` / `delete_contract_template` /
`upload_contract_template_pdf` / `publish_contract_template` / `archive_contract_template` 是**写**
（遵循合规写规则）；`get_contract_template` / `list_contract_templates` /
`list_contract_template_versions` 是认证 GET 读。**全部不需要** step-up。

`update_contract_template` 和 `delete_contract_template` 是 **DRAFT-only**——后端对 `PUBLISHED` / `ARCHIVED`
模板拒绝两者；已发布模板应改用 `archive_contract_template` 而非删除。

`upload_contract_template_pdf` 取 `{ pdf_base64 }`。SDK **不**解析 PDF、**不**校验几何、**不**在客户端算哈希——
这些都在后端做；`pdf_hash` / `pdf_page_count` 随返回的 `ContractTemplateResp` 回来。重试上传费带宽，务必持久化 key。

`list_contract_templates` 返回 `PageResult<ContractTemplatePageItem>`。列表项视图**故意省略** `fields`，
避免列表端点的大对象 N+1——字段覆盖层只在详情（`ContractTemplateResp`）和每个版本快照
（`ContractTemplateVersion`）上出现。

`list_contract_template_versions` 返回**纯数组**。每次 `publish_contract_template` 追加一条不可变
`ContractTemplateVersion`（捕获发布时的 `name` / `pdf_hash` / `fields` / `status_at_snapshot`），是该版本的
离线复核 ground truth。

## 错误分类

合规业务错误以数值 Java 错误码出现在标准 [`Error::Business`] 的 `code` 字段。SDK 把这些码映射到符号化的
[`ComplianceErrorKey`]（枚举，PascalCase 变体）：

```rust,no_run
use acosmi::classify_compliance_error;
use acosmi::compliance::ComplianceErrorKey;
# fn handle(err: &acosmi::Error) {
let info = classify_compliance_error(err);
match info.key {
    ComplianceErrorKey::ComplianceStepUpRequired => { /* 引导重认证 */ }
    ComplianceErrorKey::EnvelopeGateClosed | ComplianceErrorKey::ProviderNotConfigured => { /* 展示终态 */ }
    _ => {}
}
# }
```

`classify_compliance_error(err)` 返回 [`ComplianceErrorInfo`]，字段：`code`（i64）、`message`（String）、
`key`（`ComplianceErrorKey`）、`retryable`（bool）、`terminal`（bool）、`step_up_required`（bool）。
非合规段位的业务错误其 `key` 为 `ComplianceErrorKey::UnknownComplianceError`；用
`is_compliance_business_error(err)` 先判定一个错误是否落在合规码段。

[`CompliancePollError`] 由轮询助手用于终态失败、超时、取消、未知态（其 `kind` 是 `CompliancePollErrorKind`）。

`compliance_error_to_retry_advice(info)` 把 `ComplianceErrorInfo` 投影成跨域 [`RetryAdvice`] 模型
（`retryable` / `retry_after` / `same_idempotency_key_required` / `manual_action_required` / `reason` /
`user_message` / `developer_message` / `support_code`）。它是**加性、只读**投影——不修改也不替换
`ComplianceErrorInfo`，`classify_compliance_error` 不变。终态错误建议换新 idempotency-key
（`same_idempotency_key_required: false`）；step-up 错误建议重认证后用**同一** key 重试。

```rust,no_run
use acosmi::{classify_compliance_error, compliance_error_to_retry_advice};
# fn advise(err: &acosmi::Error) {
let info = classify_compliance_error(err);
let advice = compliance_error_to_retry_advice(&info);
if advice.manual_action_required {
    // 需人工介入（重认证 / 对账）。
}
# }
```

## 方法成熟度

每个 `client.compliance().*` 方法有四档成熟度之一。把此表当**契约**——`gated` 方法在后端 step-up / 闸门
开放前预期 fail-closed，SDK 对它们**永不**重试、**永不**伪造成功。

| 档位 | 方法 | 含义 |
| --- | --- | --- |
| `production-ready` | `create_evidence_asset` `get_evidence_asset` `verify_evidence_public` `list_evidence_assets` `list_evidence_packages` `issue_timestamp` `issue_timestamp_for_asset` `get_timestamp` `verify_timestamp` `wait_for_timestamp_verified` `list_timestamps` `list_tsa_providers` `get_tsa_stats` `build_evidence_package` `create_report` `get_report` `download_report` `list_reports` `create_signing_envelope` `get_signing_envelope` `sync_signing_envelope_status` `list_signing_envelopes` `list_envelope_contracts` `list_envelope_provider_requests` `void_envelope` `create_contract_template` `update_contract_template` `delete_contract_template` `get_contract_template` `list_contract_templates` `upload_contract_template_pdf` `publish_contract_template` `archive_contract_template` `list_contract_template_versions` `submit_seal_approval` `reject_seal_approval` `cancel_seal_approval` `list_pending_seal_approvals` `get_seal_approval` `list_seal_approvals` `list_seal_uses` `get_provider_request` `wait_for_provider_request_terminal` `get_capabilities` `get_feature_gate` `list_operations` `get_operation` `classify_error` | 后端 endpoint、scope、DTO 契约、SDK 测试与文档全部收口。可生产调用。 |
| `gated` | `publish_report` `sign_envelope` `create_h5_signing_url` `approve_seal_approval` | SDK 暴露方法，但后端在 step-up 和 W3 闸门链就绪前 fail-closed（`COMPLIANCE_STEP_UP_REQUIRED` / `ENVELOPE_GATE_CLOSED`）。SDK 不重试、不伪成功——把类型化错误呈现为「功能尚未开放」。 |
| `draft contract` | 二进制下载助手 | 仅类型草案——本版**不**暴露为可调用能力。 |
| `internal-only` | 分销计费（`reserve` / `commit` / `cancel` / `reconcile` / `refund`）、provider 原始报文、provider 回调、CFCA 受控材料 | 仅服务端 S2S。**永不**进入 SDK 调用面；没有对应 SDK 方法。 |

`submit_seal_approval` 是 `production-ready`：后端用 `Idempotency-Key` + 业务指纹做重放保护，同一 key 重复提交
返回原审批 id 而非建副本。在调用方侧持久化 key。

`get_capabilities` / `get_feature_gate` / `list_operations` / `get_operation` 对 S2（`G2`）契约
`production-ready`——它们是只读 GET 投影，自身不携带 step-up / 闸门状态；`get_capabilities` *报告* gated 动作
当前是否可执行。`list_tsa_providers` / `get_tsa_stats`（S3 `G3`）、`list_envelope_contracts` /
`list_envelope_provider_requests` / `void_envelope`（S4 `G4`）、9 个合同模板方法（S5 `G5`）、
`list_seal_uses`（S6 `G6`）同样对各自契约 `production-ready`。

## 安全边界

**不要**把以下任何材料放进 SDK 代码、测试、示例、文档、git 历史、环境模板、发布 tarball：

- provider endpoint 或 provider 原始请求/响应报文。
- 证书、私钥、keystore、签名容器、口令。
- provider 商品 id、provider 用户 id、交易码、项目码、provider 印章 id。
- 合同原文、PII、storage bucket/key、主体快照、回调扣费 commit 报文。

Java 合规后端拥有 provider 集成、受控材料、本地验证、计费状态机、OAuth/JWKS 校验。
Go OAuth/JWKS 层拥有 token 签发与内省。Rust SDK 只申请 scope、发送 Acosmi 公开 DTO、分类公开错误、
轮询安全的公开状态视图。

## 打包示例

crate 包含这些 `cargo` 示例（CI `cargo build --examples` 编译验证）：

- [`examples/compliance_read.rs`](../examples/compliance_read.rs) — 只读查询 + 公开 verify。
- [`examples/compliance_timestamp.rs`](../examples/compliance_timestamp.rs) — 证据 + 时间章 + 报告链路。
- [`examples/compliance_envelope.rs`](../examples/compliance_envelope.rs) — 签署 envelope + step-up / gate 错误处理。

它们要求调用方提供环境变量，**不**含真实 endpoint、密钥、provider 材料或原始报文。

---

完整 API 参考（每方法的精确签名 + `// 写` / `// step-up` / `// gate` 标注）见 docs.rs 的
[`compliance` 模块](https://docs.rs/acosmi-sdk/latest/acosmi/compliance/index.html)。
