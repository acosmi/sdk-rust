//! quickstart_chat — Client 基础用法：构造 / 登录 / 列模型 / 同步 chat / 流式 chat。
//!
//! 端口自 `acosmi-sdk-ts/examples/core-chat.ts`。
//!
//! 演示：
//!   1. 用 `Config` + `FileTokenStore` 构造并预载持久化 token（`Client::create`）。
//!   2. OAuth 登录（按业务最小集合申请 scope）—— 需 `desktop-loopback` feature 才会真正弹浏览器。
//!   3. 列举托管模型 + 查看配额摘要。
//!   4. 同步 `chat` 调用，遍历内容块 + usage。
//!   5. 流式 `chat_stream_with_usage` 并聚合 content / settle 事件。
//!
//! 环境变量：
//!   - `ACOSMI_SERVER_URL`（必填）：网关 base URL，例如 `https://acosmi.com`。
//!   - `ACOSMI_TOKEN_FILE`（可选）：token 持久化路径，缺省 `./quickstart-tokens.json`。
//!
//! 说明：SDK 自动按 `ManagedModel.preferred_format` 选 Anthropic / OpenAI adapter，调用方无需关心。
//!
//! 运行：`cargo run --example quickstart_chat`（CI 仅 `cargo build --example quickstart_chat`）。

use std::sync::Arc;

use acosmi::{
    all_scopes, ChatMessage, ChatRequest, ChatUsageEvent, Client, Config, FileTokenStore,
};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_url = std::env::var("ACOSMI_SERVER_URL")
        .expect("ACOSMI_SERVER_URL is required (例如 https://acosmi.com)");
    let token_file =
        std::env::var("ACOSMI_TOKEN_FILE").unwrap_or_else(|_| "./quickstart-tokens.json".into());

    // 1) 构造 Client。`Client::create` 从 store 异步预载已持久化的 token。
    let store = Arc::new(FileTokenStore::new(Some(token_file.into())));
    let client = Client::create(Config {
        server_url: Some(server_url),
        store: Some(store),
        ..Default::default()
    })
    .await?;

    // 2) OAuth 登录 —— 已有有效 token 时复用；否则走 PKCE loopback（需 `desktop-loopback` feature）。
    //    `all_scopes()` 申请全量预设 scope；生产应按业务最小集合裁剪。
    let scopes = all_scopes();
    client
        .login("Quickstart Chat Example", &scopes, None)
        .await?;

    // 3) 列举托管模型 + 配额摘要。include_locked=false → 仅返回当前账户可用模型。
    let models = client.list_models(None, false).await?;
    println!("[models] {} available", models.len());
    for m in models.iter().take(5) {
        println!(
            "  - {} (provider={}, enabled={})",
            m.model_id, m.provider, m.is_enabled
        );
    }

    let quota = client.get_quota_summary(None).await?;
    println!(
        "[quota] free_total_etu={} paid_total_etu={}",
        quota.free_total_etu, quota.paid_total_etu
    );

    // 选一个模型：优先 is_default，否则取第一个启用的。
    let model = models
        .iter()
        .find(|m| m.is_default.unwrap_or(false) && m.is_enabled)
        .or_else(|| models.iter().find(|m| m.is_enabled))
        .ok_or("no enabled model available —— 让管理员在网关启用一个模型")?;
    println!("[selected model] {}", model.model_id);

    // 4) 同步 chat 调用。
    let req = ChatRequest {
        messages: Some(vec![ChatMessage {
            role: "user".into(),
            content: "用一句话介绍 Rust。".into(),
        }]),
        max_tokens: Some(256),
        ..Default::default()
    };
    let resp = client.chat(&model.model_id, &req, None).await?;
    for block in &resp.content {
        if block.r#type == "text" {
            if let Some(text) = &block.text {
                println!("[chat text] {text}");
            }
        }
    }
    println!(
        "[chat usage] input={} output={}",
        resp.usage.input_tokens, resp.usage.output_tokens
    );

    // 5) 流式调用 —— chat_stream_with_usage 把内容 / 来源 / 结算事件分流为带标签的迭代项。
    let stream_req = ChatRequest {
        messages: Some(vec![ChatMessage {
            role: "user".into(),
            content: "写一首关于海的两行短诗。".into(),
        }]),
        max_tokens: Some(512),
        ..Default::default()
    };
    let stream = client.chat_stream_with_usage(&model.model_id, &stream_req, None);
    futures::pin_mut!(stream);
    while let Some(item) = stream.next().await {
        match item? {
            ChatUsageEvent::Content(ev) => {
                if ev.event == "content_block_delta" {
                    print!("."); // 实际项目里在此解析 delta 输出 token
                }
            }
            ChatUsageEvent::Settle(s) => {
                println!(
                    "\n[settle] total_tokens={} token_remaining={}",
                    s.total_tokens, s.token_remaining
                );
            }
            ChatUsageEvent::Sources(_) => {}
        }
    }
    println!("\n[stream] done");

    Ok(())
}
