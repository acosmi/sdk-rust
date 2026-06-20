//! agent_runs_stream — Agent Run Gateway：创建 run + 流式消费 SSE 事件。
//!
//! 端口自 `acosmi-sdk-ts/examples/agent-runs-stream.ts`。
//!
//! 演示：
//!   1. 创建一个 agent run（`client.agent_runs().create(...)`）。
//!   2. 流式消费 run 事件（`AgentRunsClient::stream`），断线可 durable replay。
//!   3. 显式 opt-in 本地只读工具桥：处理 `LocalToolRequest`，回写 `submit_local_tool_result`。
//!   4. 下载产物（`download_artifact`）。
//!   5. 处理 usage / settle 事件。
//!
//! 红线：
//!   - 下游产品禁止直连内部 chat 端点实现智能体循环，必须走 `agent_runs()`。
//!   - `LocalToolRequest` 只定义协议；handler 由下游显式提供，allowed_tools 用稳定 ASCII function name。
//!   - 结算只用 provider/ADK 透传的精确 usage；exact != true 时服务端释放 hold，不估算扣费。
//!
//! 环境变量：
//!   - `ACOSMI_SERVER_URL`（必填）：网关 base URL。
//!
//! 运行：`cargo run --example agent_runs_stream`（CI 仅 `cargo build --example agent_runs_stream`）。

use acosmi::{
    all_scopes, AgentRunArtifactPolicy, AgentRunCreateRequest, AgentRunLocalContextPolicy,
    AgentRunLocalToolResult, AgentRunStreamEvent, AgentRunStreamOptions, Client, Config,
};
use futures::StreamExt;
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_url = std::env::var("ACOSMI_SERVER_URL").expect("ACOSMI_SERVER_URL is required");

    let client = Client::create(Config {
        server_url: Some(server_url),
        ..Default::default()
    })
    .await?;
    let scopes = all_scopes();
    client.login("Agent Runs Example", &scopes, None).await?;

    let agent_runs = client.agent_runs();

    // 1) 创建 run。create 是 POST 副作用操作，401 不自动 refresh 重放。
    let create_req = AgentRunCreateRequest {
        app_id: "crabdesign".into(),
        mode: Some("design".into()),
        input: "Create a landing page mockup for a fintech dashboard".into(),
        active_skill_ids: Some(vec!["brand-system".into()]),
        knowledge_base_ids: Some(vec!["kb-product".into()]),
        // 本地只读上下文策略：显式 opt-in，限制可用工具与读取上限。
        local_context_policy: Some(AgentRunLocalContextPolicy {
            enabled: Some(true),
            readonly: Some(true),
            max_bytes: Some(128_000),
            allowed_tools: Some(vec!["read_file".into()]),
        }),
        artifact_policy: Some(AgentRunArtifactPolicy {
            enabled: Some(true),
            max_files: Some(10),
        }),
        ..Default::default()
    };
    let run = agent_runs.create(&create_req, None).await?;
    println!(
        "[run created] run_id={} status={}",
        run.run_id,
        run.status.as_str()
    );

    // 2) 流式消费。stream 支持 durable replay：断线后重连同一 run 会先回放已持久化事件。
    //    throw_on_error=false → 自行消费 error 事件而不是直接抛 Error::AgentRunStream。
    let opts = AgentRunStreamOptions {
        throw_on_error: false,
    };
    let stream = agent_runs.stream(&run.run_id, opts, None);
    futures::pin_mut!(stream);
    while let Some(event) = stream.next().await {
        match event? {
            AgentRunStreamEvent::TextDelta { text } => print!("{text}"),
            AgentRunStreamEvent::ReasoningDelta { text } => println!("\n[reasoning] {text}"),
            AgentRunStreamEvent::LocalToolRequest {
                request_id,
                name,
                input,
            } => {
                // 3) 本地工具桥 —— 由下游产品代码拥有，这里只是只读演示实现。
                let result = handle_local_tool(&name, &input);
                agent_runs
                    .submit_local_tool_result(
                        &run.run_id,
                        &AgentRunLocalToolResult {
                            request_id,
                            ok: result.ok,
                            content: result.content,
                            error: result.error,
                        },
                        None,
                    )
                    .await?;
            }
            AgentRunStreamEvent::Artifact { artifact } => {
                // 4) 下载产物 —— GET 安全查询，允许单次 401 refresh 重试。
                let file = agent_runs
                    .download_artifact(&run.run_id, &artifact.id, None)
                    .await?;
                println!(
                    "\n[artifact] {} {} {} bytes",
                    file.filename,
                    file.content_type.unwrap_or_default(),
                    file.data.len()
                );
            }
            AgentRunStreamEvent::Usage { usage } => {
                println!(
                    "\n[usage] total_tokens={:?} exact={:?}",
                    usage.total_tokens, usage.exact
                );
            }
            AgentRunStreamEvent::Settle { settlement } => {
                println!(
                    "[settle] status={:?} token_remaining={:?}",
                    settlement.status, settlement.token_remaining
                );
            }
            AgentRunStreamEvent::Error { error } => {
                eprintln!(
                    "\n[error] {} {}",
                    error.code.unwrap_or_default(),
                    error.message
                );
            }
            AgentRunStreamEvent::Done { status, .. } => println!("\n[done] status={status}"),
            other => {
                // run_started / status / tool_call / tool_result / sources 等其余事件。
                println!("\n[event] {}", other.type_str());
            }
        }
    }

    // 5) cancel 可从 UI 取消按钮安全调用（即使 run 已结束也不会抛）。
    // agent_runs.cancel(&run.run_id, None).await?;

    Ok(())
}

struct LocalToolOutcome {
    ok: bool,
    content: Option<Value>,
    error: Option<String>,
}

/// 本地只读工具实现示例。生产环境只暴露受控的只读操作，拒绝越权路径。
fn handle_local_tool(name: &str, input: &Value) -> LocalToolOutcome {
    if name != "read_file" {
        return LocalToolOutcome {
            ok: false,
            content: None,
            error: Some(format!("local tool rejected: unsupported tool {name}")),
        };
    }
    // 这里应做路径白名单校验后读取文件；示例仅返回占位内容。
    LocalToolOutcome {
        ok: true,
        content: Some(json!({ "note": "read-only local context placeholder", "input": input })),
        error: None,
    }
}
