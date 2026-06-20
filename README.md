# acosmi-sdk (Rust)

> Acosmi 模型网关 + Agent Run Gateway + Compliance 的 Rust SDK — 双格式（Anthropic + OpenAI），原生异步（`tokio` + `reqwest`/rustls）。

[![crates.io](https://img.shields.io/crates/v/acosmi-sdk.svg)](https://crates.io/crates/acosmi-sdk)
[![docs.rs](https://img.shields.io/docsrs/acosmi-sdk)](https://docs.rs/acosmi-sdk)

## 状态

- 端口自 [`@acosmi/sdk-ts`](https://github.com/acosmi/sdk-ts)（事实标准主实现）。当前对齐 **v2.8.0**，18 业务域全覆盖。
- 仅原生运行时（`tokio` + `reqwest`，rustls TLS）；不提供 WASM/浏览器并列构建。
- 跨语言契约（snake_case wire-format / 符号名对齐 / bug-for-bug 行为）见 [`docs/开发与发布手册.md`](./docs/开发与发布手册.md) §5。
- API 参考由 `cargo doc` / [docs.rs](https://docs.rs/acosmi-sdk) 从 `///` 自动生成（Rust 生态惯例，无手写 API 目录）。

## 安装

```toml
[dependencies]
acosmi-sdk = "2.8"
tokio = { version = "1", features = ["full"] }
```

库名为 `acosmi`：

```rust
use acosmi::{Client, Config};
```

可选 feature：

| feature | 默认 | 说明 |
|---------|------|------|
| `sanitize` | ✅ | 历史消息清洗子包（对齐 npm `./sanitize`） |
| `desktop-loopback` | — | 桌面 OAuth 的 loopback HTTP server（`authorize()`） |

## 快速开始

```rust,no_run
use acosmi::{Client, Config, all_scopes};
use acosmi::models::{ChatRequest, ChatMessage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Config 实现 Default；server_url 缺省为 DEFAULT_GATEWAY_BASE_URL（https://acosmi.com）。
    let client = Client::new(Config {
        server_url: Some(std::env::var("ACOSMI_BASE_URL")?),
        ..Default::default()
    })?;

    // 内置 loopback OAuth（feature `desktop-loopback`）或先用 set/login 持有 token。
    // login 接 &[String]，all_scopes() 返回 Vec<String>。
    client.login("My App", &all_scopes(), None).await?;

    // ChatMessage 是扁平 struct { role, content }（wire 契约，无构造器）。
    let req = ChatRequest {
        messages: Some(vec![ChatMessage { role: "user".into(), content: "Hello".into() }]),
        max_tokens: Some(1024),
        // end_user_id（v1.6.0+）= 业务侧稳定 id，启用上游隔离 / KV-cache / 调度策略。
        end_user_id: Some("user-abc-123".into()),
        ..Default::default()
    };

    let resp = client.chat("claude-opus-4-7", &req, None).await?;
    println!("{:?}", resp.content);
    Ok(())
}
```

> 各域端到端可运行示例见 [`examples/`](./examples/)（`cargo run --example quickstart_chat` 等）。

### Acosmi Gateway URL — `server_url` 公共契约

`Config.server_url` 是 SDK 调 Acosmi nexus-v4 API 的根地址：

- 只接受 `http:` / `https:`；`ws:` / `wss:` 是 CrabCode `--sdk-url` RemoteIO 会话通道，**不是** SDK gateway URL，传入立刻抛错。
- agent-runs / managed-models / notifications WS / compliance 都从同一个 normalized base 派生；内部 `api_url()` 自动追加 `/api/v4`，不会重复拼。
- `compliance_base_url` 是独立第二根地址，不被 `server_url` 覆盖。

```rust,no_run
use acosmi::{Client, Config, DEFAULT_GATEWAY_BASE_URL};
use acosmi::core::normalize_gateway_base_url; // root 仅导出 DEFAULT_GATEWAY_BASE_URL

normalize_gateway_base_url("https://gw.example/api/v4/")?; // → "https://gw.example/api/v4"
normalize_gateway_base_url("wss://session.example").unwrap_err(); // 仅允许 http/https

assert_eq!(DEFAULT_GATEWAY_BASE_URL, "https://acosmi.com");

let c = Client::new(Config { server_url: Some("https://gw.example".into()), ..Default::default() })?;
assert_eq!(c.base_url(), "https://gw.example"); // = c.server_url()
c.api_url("/agent-runs"); // → "https://gw.example/api/v4/agent-runs"
# Ok::<(), Box<dyn std::error::Error>>(())
```

### 用户隔离（end_user_id）

`ChatRequest.end_user_id` — 业务侧终端用户的稳定标识，**跨 provider 通用语义**。SDK 自动按 wire-format 注入（OpenAI 顶层 `user_id` / Anthropic `metadata.user_id`），网关侧校验并派生后命中三项隔离能力（内容安全 / KV-cache / 调度）。

**约束**：字符集 `[a-zA-Z0-9_-]+`，长度 ≤ 512，**禁止包含 PII**（邮箱 / 手机 / 真名等）。用 `validate_end_user_id(s)` 自校验：

```rust
use acosmi::validate_end_user_id;

// 返回 Option<String>：Some(错误信息) = 不合规，None = 合法。
if let Some(err) = validate_end_user_id(Some("user-abc-123")) {
    eprintln!("非法 end_user_id: {err}");
}
```

不传时网关从认证身份 HMAC-SHA256 自动派生 32 字符稳定 id，业务无感知。流式 + 同步 + Anthropic + OpenAI 四条路径均支持。

## 双格式红线（设计核心）

SDK 同时提供 **Anthropic + OpenAI 两条 endpoint**，**等地位**，对应两个不同下游产品。

| Adapter | 端点 | 用途 |
| --- | --- | --- |
| `Adapter::Anthropic` | `POST /managed-models/:id/anthropic` | Anthropic 原生格式（含 thinking 等） |
| `Adapter::OpenAI` | `POST /managed-models/:id/chat` | OpenAI 兼容格式（DeepSeek/GLM 等） |

路由由 `get_adapter_for_model(model)` 按 `ManagedModel` 的 `preferred_format` / `supported_formats`（wire snake_case，与上游 Go json tag 严格对齐）决策：

1. `preferred_format` 非空 **且**该格式在 `supported_formats` 内（或 `supported_formats` 未声明）→ 按值（`anthropic` | `openai`）
2. `supported_formats` 含 `anthropic` → `Adapter::Anthropic`
3. `supported_formats` 含 `openai` → `Adapter::OpenAI`
4. 两字段均空（旧上游）→ 按 `provider` 名回落

> **格式一致性护栏（v2.5.1）**：第 1 步收紧后，`preferred_format` 与 `supported_formats` 矛盾时（如 `preferred_format=anthropic` 但 `supported_formats=[openai]`）不再盲信 `preferred_format`，而落到第 2/3 步按实际支持的格式选择，避免路由到模型并不支持的端点撞 4xx。`supported_formats` 未声明（旧上游）时 `preferred_format` 仍直接采信，向后兼容。

`client.chat()` / `client.chat_stream()` 内部自动调 `get_adapter_for_model`，使用方无需关心。

## 图片 / 视频生成（托管模型网关）

图片、视频生成与文本对话**同属托管模型网关**（同一个 `Client`、同一套 `models:chat` 鉴权面），**不是工作流**。只有 `capabilities.supports_image_generation` / `supports_video_generation` 为真的模型可调。

**先按 capability 筛模型**（严禁用模型名 substring 推断）：

```rust,no_run
# async fn demo(client: &acosmi::Client) -> Result<(), Box<dyn std::error::Error>> {
let models = client.list_models(None, false).await?; // (signal, include_locked)
let image_model = models.iter().find(|m| m.capabilities.supports_image_generation);
let video_model = models.iter().find(|m| m.capabilities.supports_video_generation);
# Ok(()) }
```

- **图片（同步）**：`client.generate_image(model_id, &ImageGenerationRequest{..}, None)` 一次调用直接拿图（内部超时与 chat 同级 11min）。
- **视频（异步）**：`client.generate_video(...)` 返回 `task_id`，再用 `client.poll_video_task(model_id, task_id, Some(duration), None)` 轮询到 `completed`。`duration` 务必回传创建时的秒数——网关据此上报真实视频时长用量。

## 向量 / 重排序（托管模型网关，v2.9+）

向量（embedding）与重排序（rerank）与 chat **同网关、同会员计费**（Hold→Settle→Release，按 `total_tokens` 套 input 费率），上游接阿里云百炼 DashScope。只有 `capabilities.supports_embedding` / `supports_rerank` 为真的模型可调；具体上游模型名由管理员在后台自填。

```rust,no_run
# use acosmi::{EmbeddingRequest, EmbeddingInput, RerankRequest};
# async fn demo(client: &acosmi::Client) -> Result<(), Box<dyn std::error::Error>> {
let models = client.list_models(None, false).await?;

// 向量（同步）
if let Some(m) = models.iter().find(|m| m.capabilities.supports_embedding == Some(true)) {
    let resp = client.embeddings(&m.id, &EmbeddingRequest {
        input: Some(EmbeddingInput::Batch(vec!["第一段".into(), "第二段".into()])),
        dimensions: Some(1024),
        ..Default::default()
    }, None).await?;
    println!("{} 维, {} tokens", resp.data[0].embedding.len(), resp.usage.total_tokens);
}

// 重排序（同步）
if let Some(m) = models.iter().find(|m| m.capabilities.supports_rerank == Some(true)) {
    let resp = client.rerank(&m.id, &RerankRequest {
        query: "什么是文本排序模型".into(),
        documents: vec!["文档A".into(), "文档B".into()],
        top_n: Some(2),
        return_documents: Some(true),
        instruct: None,
        fps: None,
    }, None).await?;
    for r in &resp.results {
        println!("#{} score={} {:?}", r.index, r.relevance_score, r.document);
    }
}
# Ok(()) }
```

> 重排序对外是统一扁平契约；网关内部按模型绑定线路（原生嵌套 `gte-rerank-v2` / OpenAI 兼容扁平 `qwen3-rerank`）自动转换并归一化响应。

### 多模态向量 / 重排序（v2.10+，text / image / video）

对接 DashScope `qwen3-vl-embedding`（多模态向量）与 `qwen3-vl-rerank`（多模态重排序）。向量用 `contents` 取代 `input`；重排序的 `query` / `documents` 接受多模态对象 `MultimodalContent { text?, image?, video? }`（也可混入纯文本字符串）。适用于自建搜索引擎的图文 / 视频检索。

```rust,no_run
# use acosmi::{EmbeddingRequest, RerankRequest, MultimodalContent};
# async fn demo(client: &acosmi::Client, mm_emb: &str, mm_rerank: &str) -> Result<(), Box<dyn std::error::Error>> {
// 多模态向量：图 / 视频 / 文本混合
let emb = client.embeddings(mm_emb, &EmbeddingRequest {
    contents: Some(vec![
        MultimodalContent { text: Some("一只橘猫".into()), ..Default::default() },
        MultimodalContent { image: Some("https://…/cat.png".into()), ..Default::default() },
        MultimodalContent { video: Some("https://…/clip.mp4".into()), ..Default::default() },
    ]),
    output_type: Some("dense".into()),
    fps: Some(2.0),
    ..Default::default()
}, None).await?;
println!("{} 维", emb.data[0].embedding.len());

// 多模态重排序：query 与候选可为多模态对象，也可混入纯文本字符串
let rr = client.rerank(mm_rerank, &RerankRequest {
    query: MultimodalContent { text: Some("红色跑车".into()), ..Default::default() }.into(),
    documents: vec![
        MultimodalContent { image: Some("https://…/car.png".into()), ..Default::default() }.into(),
        "一段描述文字".into(),
    ],
    top_n: Some(5),
    return_documents: None,
    instruct: None,
    fps: Some(1.5),
}, None).await?;
println!("{} 条", rr.results.len());
# Ok(()) }
```

> 文本调用完全向后兼容：`input` / 字符串 `query` / 字符串 `documents`（经 `.into()`）行为不变。多模态托管模型须由管理员在后台勾选对应能力位与输入模态（text/image/video）。

## 流式

流式返回 `impl Stream`，用 `futures::StreamExt` 消费；取消走 `tokio_util::sync::CancellationToken`（取代 TS `AbortSignal`）。

```rust,no_run
use futures::StreamExt;
use acosmi::models::StreamEvent;

# async fn demo(client: &acosmi::Client, req: &acosmi::models::ChatRequest) -> Result<(), Box<dyn std::error::Error>> {
let mut stream = Box::pin(client.chat_stream("claude-opus-4-7", req, None));
while let Some(ev) = stream.next().await {
    let ev: StreamEvent = ev?;
    // 解析 content_block_delta 输出 token
}
# Ok(()) }
```

`chat_stream_with_usage()` 返回带 `Settle`（结算）/ `Sources`（搜索来源）/ `Content`（内容增量）标签的 `impl Stream<Item = Result<ChatUsageEvent>>`，便于聚合统计。

> **红线**：流式路径**永不重试**（防双扣）；POST 默认不重试（计费安全）；401 单次重试防递归。

## Agent Runs

`client.agent_runs()` 是下游产品（CrabDesign / CrabCode / CrabClaw 等）接入 Acosmi 云端智能体循环的正式 SDK 边界，不要直连 Nexus 内部 `/api/v4/chat/completions`。服务端按 `tenant_id + user_id` 隔离，run 状态 / SSE event / artifact / local tool result 均 durable store，执行进入统一 entitlement 预扣/结算/释放链路。

```rust,no_run
use futures::StreamExt;
use acosmi::agent_runs::{AgentRunCreateRequest, AgentRunStreamEvent};

# async fn demo(client: &acosmi::Client) -> Result<(), Box<dyn std::error::Error>> {
let runs = client.agent_runs();
let run = runs.create(&AgentRunCreateRequest {
    app_id: "crabdesign".into(),
    input: "Create a landing page mockup".into(),
    ..Default::default()
}, None).await?;

// stream(run_id, opts: AgentRunStreamOptions /* 传值 */, signal)
let mut stream = Box::pin(runs.stream(&run.run_id, Default::default(), None));
while let Some(ev) = stream.next().await {
    match ev? {
        AgentRunStreamEvent::TextDelta { text } => print!("{text}"),
        AgentRunStreamEvent::Error { error } => return Err(error.message.into()),
        _ => {}
    }
}
# Ok(()) }
```

本地工具桥是显式 opt-in：SDK 只定义协议，`local_tool_request` 由下游处理，结果通过 `submit_local_tool_result` 回填。`stream(run_id)` 支持 durable replay：断线重连同一 run 会先回放已持久化事件再继续。完整事件 union 见 docs.rs 的 [`agent_runs::AgentRunStreamEvent`](https://docs.rs/acosmi-sdk/latest/acosmi/agent_runs/enum.AgentRunStreamEvent.html)。

### 远程控制 — CrabCode remote-control

远程控制是 Agent Run 的独立 runtime（`runtime: "crabcode_remote"`），事件协议另成一套（契约 §4 的 11 事件 `RemoteControlEvent`）。`create_remote_run` + `stream_remote_control`；`error` 恒为非终结，`done` / `settle` 才终结流，用 `is_terminal_remote_event(ev)` 判终结。远控是高风险 scope，**不在** `all_scopes()` 内，用 `remote_control_scopes()` 显式申请。

### BYOK — 用户自有模型密钥

`CrabCodeByokClient`（守卫 `remote_control` scope）管理 BYO 模型密钥；明文一次性提交、服务端加密落库后即弃，所有读取只回 masked 视图（ref + fingerprint）。创建后把 `credential_ref` 传给 `create_remote_run`（仅 `runner: cloud`）。

## Chat Bridge（第三方聊天平台桥接）

`client.chat_bridge()` 提供第三方聊天平台（飞书/企微/钉钉/Slack/Teams/Telegram/WhatsApp）集成与凭证的管理面 CRUD。云端控制台与下游调的是同一组端点、同一份按租户隔离的数据，任一端创建/修改另一端即时可见。

**安全红线**：平台 secret 只在 `store_credential` / `rotate_credential` 请求体出现一次，服务端加密落库后即弃；SDK 公共面只见 `CredentialRef`（`cred_<base32>`）+ `fingerprint` + 脱敏 metadata，`ChatCredentialPublic` **编译期即无密文字段**。chat_bridge 是高风险 scope，不在 `all_scopes()` 内，用 `chat_bridge_scopes()` 显式申请。

## 认证

### 内置 Loopback OAuth（推荐）

```rust,no_run
# async fn demo(client: &acosmi::Client) -> Result<(), Box<dyn std::error::Error>> {
client.login("My App", acosmi::all_scopes(), None).await?; // 自动跳转浏览器完成 OAuth
let token = client.ensure_token(None).await?;              // 拿到当前有效 access token
# Ok(()) }
```

### 手动 OAuth（CLI / 自定义流程）

底层自由函数适用于自管 token 的 CLI / 自定义授权 UI。完整可运行示例见 [`examples/oauth_flow.rs`](./examples/oauth_flow.rs)。

```rust,no_run
use acosmi::{discover, register, exchange_code, new_token_set,
             generate_code_verifier, code_challenge, FileTokenStore, TokenStore};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
// 自由函数走显式注入的 reqwest::Client（取代 TS 隐式 fetchImpl）。
let http = reqwest::Client::new();
let server = std::env::var("ACOSMI_SERVER_URL")?;
let meta = discover(&http, &server).await?;            // RFC 8414 元数据发现
let reg = register(&http, &meta, "My CLI").await?;     // RFC 7591 动态客户端注册

let verifier = generate_code_verifier();
let challenge = code_challenge(&verifier);             // S256 = base64url(SHA-256(verifier))
// …引导用户浏览器授权（用 challenge），回调拿到 code 与 redirect_uri…
# let (code, redirect_uri) = ("code".to_string(), "http://127.0.0.1/cb".to_string());
# let _ = challenge;

let resp = exchange_code(&http, &meta, &reg.client_id, &code, &redirect_uri, &verifier).await?;
let tokens = new_token_set(&resp, &reg.client_id, &server);
FileTokenStore::new(Some("./tokens.json".into())).save(&tokens).await?;
# Ok(()) }
```

### Token 持久化

`TokenStore` trait 有两个内置实现 + 可自定义：

| 实现 | 用途 |
| --- | --- |
| `FileTokenStore` | 落盘（缺省 `~/.acosmi/tokens.json`，`save()` 真 fsync） |
| `InMemoryTokenStore` | 进程内存（不持久化，高安全/测试场景） |
| `impl TokenStore` | 自定义后端（如 OS keychain） |

```rust,no_run
use acosmi::{Client, Config, FileTokenStore};
use std::sync::Arc;

# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::new(Config {
    server_url: Some("https://acosmi.com".into()),
    // FileTokenStore::new(Option<PathBuf>)；None = 缺省 ~/.acosmi/tokens.json。
    store: Some(Arc::new(FileTokenStore::new(Some("./my-tokens.json".into())))),
    ..Default::default()
})?;
# let _ = client; Ok(()) }
```

## API 总览（18 业务域）

每个业务域是该域对外切片的单一真相源，经 `lib.rs` 逐域 re-export；完整签名见 [docs.rs](https://docs.rs/acosmi-sdk)，IDE 自带补全。

| 域 | 入口 | 主要能力 |
| --- | --- | --- |
| **core** | `Client` | 构造 / 认证 / 模型列举 / chat（同步+流式）/ 图片视频生成 / 向量+重排序 |
| **models** | `models::*` | 双 adapter 选路 / ManagedModel catalog / web search tool / 能力 helper |
| **auth** | 自由函数 | OAuth 2.1 PKCE 原语 / scope helper / TokenSet |
| **agent_runs** | `Client::agent_runs()` | 云端智能体 run / 流式 / 本地工具桥 / 远程控制 / BYOK |
| **chatbridge** | `Client::chat_bridge()` | 第三方聊天平台集成 / 凭证管理面 CRUD |
| **compliance** | `Client::compliance()` | 电子证据 / 时间章 / 出证报告 / 签署 envelope / 用印审批 / 合同模板 |
| **billing** | `Client` | 钱包 / 余额权益 / 流量包购买 / 消费记录 |
| **skills** | `Client` | 技能商店浏览 / 安装下载 / 生成优化 / 认证 |
| **notifications** | `Client` | 通知列表 / 偏好 / 设备 / WebSocket 实时推送 |
| **subscription** | `Client` | 会员订阅查询 / 档位 / 升级前置校验 |
| **pricing** / **products** | `Client` | 公开定价配置 / 合规报价 / 商品中心索引 |
| **casehall** | `Client` | 律师库 / 案件线索 / 咨询 / 法律服务 SKU |
| **enterprise** | `Client` | 企业席位 / 成员邀请 / 组织订阅 / 用量报表 |
| **finance** | `Client` | 发票 / 退款 / 对公转账（金额 `*_fen` = i64 整数分） |
| **support** | `Client` | Bug Report 提交 / 公开查看 |
| **shared** | `Error` / `Result` | 跨域错误体系 / 分页 / 幂等键 / retry-advice |

### `sanitize` 命名空间（历史消息清理，feature `sanitize`）

`sanitize` 子包对消息历史做白名单过滤 + 深度/尺寸校验 + ephemeral 剥离，经 `core::sanitize_bridge` 与 `Client` 接通。**默认零开销**——只有显式配置后才走流水线。

> **红线**：`thinking` / `redacted_thinking` 块在 Anthropic 续轮的"上一轮返回什么、下一轮就必须原样回传"硬约束下走**豁免**，禁止从历史中剔除。

## 错误处理

所有方法返回 `Result<T, Error>`（`shared::Error`，`thiserror` 顶层 enum）。用 `match` 取代 TS 的 `instanceof`：

```rust,no_run
use acosmi::Error;

# async fn demo(client: &acosmi::Client, req: &acosmi::models::ChatRequest) {
match client.chat("claude-opus-4-7", req, None).await {
    Ok(resp) => { /* … */ }
    Err(Error::Http(e)) if e.status_code == 401 => { /* 重新登录 */ }
    Err(Error::Business { code, message }) => eprintln!("业务错误 {code}: {message}"),
    Err(Error::Stream(e)) => { /* 网关流式失败事件 */ let _ = e; }
    Err(e) => eprintln!("{e}"),
}
# }
```

变体对照（TS class → Rust `Error` 变体）：

| TS 错误类 | Rust `Error` 变体 | 触发 |
| --- | --- | --- |
| `HTTPError` | `Error::Http(HttpError)` | 4xx/5xx，含 `status_code` / `r#type` / `retry_after` / `body` |
| `NetworkError` | `Error::Network(NetworkError)` | TCP/DNS/TLS 失败 / 超时（`is_timeout()` / `is_eof()`） |
| `StreamError` | `Error::Stream(StreamError)` | 网关 `managed_model_stream_failed` 事件 |
| `BusinessError` | `Error::Business { code, message }` | 网关返回 `code != 0` |
| `RateLimitError` | `Error::RateLimit` | 429 限流 |
| `OrderTerminalError` | `Error::OrderTerminal` | `wait_for_payment` 终态失败 |
| `ModelNotFoundError` | `Error::ModelNotFound` | listModels 刷新一次后仍未命中 model_id |
| `CompliancePollError` | `CompliancePollError`（compliance 域专用） | 轮询终态失败或超时 |

### 金额三阵营（务必区分，不跨阵营运算）

| 阵营 | 类型 | 适用 |
| --- | --- | --- |
| 钱包域 | `f64` | `WalletStats` / `Transaction.amount`（Go float64 端点） |
| finance / 商品化 | `i64`（整数分） | 所有 `*_fen` 字段（`amount_fen` / `tax_amount_fen` …），2^53 内无浮点风险 |
| `json.Number` 类十进制 | `String` | 上层用 `rust_decimal` 解析 |

> **额度单位双体系**：`get_balance` / `get_balance_detail` / `list_entitlements` / `get_membership` 的 `token*` 字段单位取决于权益是否付费：**免费档（TK 体系）= 原始 Token**；**付费会员（`type ∈ {TOKEN_PACKAGE, SUBSCRIPTION}`）= 微 Credits（÷1000 = Credits）**。**绝不跨单位求和**。

## Compliance（时间章 / 电子证据 / 合同签署）

合规域走独立子客户端 `client.compliance()`，使用独立 base URL（`compliance_base_url` 缺省 `${server_url}/admin-api`）。**SDK 永远不接触 provider endpoint、证书/密钥材料、provider raw payload / callback billing commit**；所有 provider 选择由服务端按配置决定，调用方不传 `provider` 字段。

```rust,no_run
use acosmi::{Client, Config, compliance_scopes};
use acosmi::compliance::{IssueTimestampRequest, ComplianceWriteOptions, CompliancePollOptions};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::create(Config {
    server_url: Some(std::env::var("ACOSMI_SERVER_URL")?),
    ..Default::default()
}).await?;
let scopes: Vec<String> = compliance_scopes().iter().map(|s| s.to_string()).collect();
client.login("My App", &scopes, None).await?;

let cc = client.compliance();
// 写操作必须持久化 idempotency-key（重启后仍可用）
let opts = ComplianceWriteOptions { idempotency_key: Some(format!("ts-{order_id}")), ..Default::default() };
# let order_id = "1";
let token = cc.issue_timestamp(&IssueTimestampRequest::default(), &opts, None).await?;
let verified = cc.wait_for_timestamp_verified(&token.id, &CompliancePollOptions::default(), None).await?;
# let _ = verified; Ok(()) }
```

**写操作幂等与 401 策略**（合规域有别于普通 API）：

- **Idempotency-Key**：所有 POST 写操作支持，调用方必须**持久化** key。同一 key 重发等价于"对账查询同一业务结果"，避免 provider 侧重复请求/扣费。
- **401 不自动重放**：写操作 401 直接抛 `Error::Http`，不自动 refresh + replay；需重新登录后用**同一 idempotency-key** 重调。GET 读操作仍走单次 401 refresh 重试。
- **5xx / timeout 不自动重试**：合规域写操作完全禁用自动重试。
- **step-up 错误**（`COMPLIANCE_STEP_UP_REQUIRED`，code=1031000013）：经 `classify_compliance_error` 识别后引导用户重新做 OAuth introspection / 升级 token 等级。

**公开 verify 匿名语义**：`verify_evidence_public` 可匿名调用（未 login 不抛 not-authorized），返回字段**不暴露** PII / 合同原文 / storage bucket+key / provider raw / TSA 证书内部字段。

> 完整合规域指南（scope / 幂等重试 / 分页 / 能力闸门 / 模板 / 错误分类 / 方法成熟度 / 安全边界）见
> [`docs/compliance.md`](./docs/compliance.md)；PII 角色可见性矩阵见
> [`docs/pii-role-matrix.md`](./docs/pii-role-matrix.md)。逐方法精确签名见 docs.rs 的
> [`compliance` 模块](https://docs.rs/acosmi-sdk/latest/acosmi/compliance/index.html)，可运行示例见
> [`examples/compliance_*.rs`](./examples/)。

## 取消（CancellationToken）

每个异步方法都接 `signal: Option<CancellationToken>`，用于取消请求或流（取代 TS `AbortSignal`）：

```rust,no_run
use tokio_util::sync::CancellationToken;

# async fn demo(client: &acosmi::Client, req: &acosmi::models::ChatRequest) -> Result<(), Box<dyn std::error::Error>> {
let token = CancellationToken::new();
let child = token.clone();
tokio::spawn(async move {
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    child.cancel();
});
let _ = client.chat("claude-opus-4-7", req, Some(token)).await;
# Ok(()) }
```

## 开发

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test                 # 单元 + 集成
cargo test --doc           # 文档示例
cargo build --examples     # 6 个端到端示例编译校验
cargo doc --no-deps        # 生成 docs.rs 同款 API 参考
```

完整开发与发布（crates.io）流程见 [`docs/开发与发布手册.md`](./docs/开发与发布手册.md)。

## 更新历史

见 [CHANGELOG.md](./CHANGELOG.md)。当前 **2.8.0**（Rust 首版，端口自 sdk-ts v2.8.0，18 域全覆盖）。

## License

[MIT](./LICENSE) — Copyright (c) 2026 Acosmi
</content>
