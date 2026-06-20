//! compliance_envelope — 签署 envelope + step-up / gate 错误处理。
//!
//! 端口自 `acosmi-sdk-ts/examples/compliance-envelope.ts`。
//!
//! 演示：
//!   1. 创建合同签署 envelope（`create_signing_envelope`，DRAFT）。
//!   2. 查询 envelope 状态（`get_signing_envelope`）。
//!   3. 正确处理 step-up / gate closed 错误（不重试、不伪成功）。
//!   4. 查询 provider request 脱敏状态（`wait_for_provider_request_terminal`；
//!      SUCCESS 不等于扣费 commit）。
//!
//! 红线：
//!   - 不传 provider 侧印章、项目或主体字段；这些由后端归一映射。
//!   - sign / h5-url 在后端闸门关闭时返回稳定错误，SDK 不重试、不伪成功。
//!   - provider success 不等于 billing committed；最终扣费状态以 envelope 业务字段为准。
//!
//! 环境变量：
//!   - `ACOSMI_SERVER_URL`（必填）：网关 base URL。
//!   - `ACOSMI_COMPLIANCE_BASE_URL`（可选）：compliance 独立 ingress。
//!   - `PROVIDER_REQUEST_ID`（可选）：演示 provider request 脱敏状态轮询；缺省跳过。
//!
//! 运行：`cargo run --example compliance_envelope`（CI 仅 `cargo build --example compliance_envelope`）。

use acosmi::compliance::ComplianceErrorKey;
use acosmi::{
    classify_compliance_error, is_compliance_business_error, Client, CompliancePollErrorKind,
    CompliancePollOptions, ComplianceWriteOptions, Config, CreateSigningEnvelopeRequest,
    SignEnvelopeRequest,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_url = std::env::var("ACOSMI_SERVER_URL").expect("ACOSMI_SERVER_URL is required");
    let compliance_base_url = std::env::var("ACOSMI_COMPLIANCE_BASE_URL").ok();

    let client = Client::create(Config {
        server_url: Some(server_url),
        compliance_base_url,
        ..Default::default()
    })
    .await?;

    let scopes: Vec<String> = vec![
        "compliance:contract_signing:read".into(),
        "compliance:contract_signing:write".into(),
        "compliance:seal_approval:request".into(),
    ];
    client.login("Envelope Example", &scopes, None).await?;

    let compliance = client.compliance();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let envelope_key = format!("envelope-{nonce}");

    // 1) 创建 envelope（DRAFT）。
    let envelope_id = compliance
        .create_signing_envelope(
            &CreateSigningEnvelopeRequest {
                envelope_no: Some(format!("EV-{nonce}")),
                request_id: Some(envelope_key.clone()),
                billing_group_id: Some(format!("BG-{nonce}")),
                ..Default::default()
            },
            &ComplianceWriteOptions {
                idempotency_key: Some(envelope_key.clone()),
            },
            None,
        )
        .await?;
    println!("[envelope created] id={envelope_id}");

    // 2) 查询 envelope 详情。
    let envelope = compliance.get_signing_envelope(envelope_id, None).await?;
    println!(
        "[envelope detail] {} status={} pending_reason={:?}",
        envelope.envelope_no, envelope.status, envelope.pending_reason
    );

    // 3) 试调用 sign —— 后端闸门关闭时会失败（ENVELOPE_GATE_CLOSED）。
    let sign_result = compliance
        .sign_envelope(
            envelope_id,
            &SignEnvelopeRequest {
                contract_hash: Some("dummy-hash".into()),
                ..Default::default()
            },
            &ComplianceWriteOptions {
                idempotency_key: Some(format!("sign-{envelope_key}")),
            },
            None,
        )
        .await;
    if let Err(e) = sign_result {
        if is_compliance_business_error(&e) {
            let info = classify_compliance_error(&e);
            if info.step_up_required {
                eprintln!("[sign] step-up required —— 引导用户重做 OAuth introspection / 重登录后用同一 idempotency-key 重试");
            } else if info.key == ComplianceErrorKey::EnvelopeGateClosed {
                eprintln!(
                    "[sign] gate closed —— 后端闸门未开放，不要重试，向用户展示\"功能开放中\""
                );
            } else if info.terminal {
                eprintln!("[sign] terminal: {:?} —— 重试无用", info.key);
            } else {
                eprintln!("[sign] business error: {:?} {}", info.key, info.message);
            }
        } else {
            eprintln!("[sign] unexpected error: {e}");
        }
    }

    // 4) 查询 provider request 状态（脱敏）。
    let provider_request_id: i64 = std::env::var("PROVIDER_REQUEST_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if provider_request_id > 0 {
        let poll_opts = CompliancePollOptions {
            timeout_ms: Some(30_000),
            ..Default::default()
        };
        match compliance
            .wait_for_provider_request_terminal(provider_request_id, &poll_opts, None)
            .await
        {
            Ok(view) => {
                println!(
                    "[provider request terminal] status={} terminal={} retryable={}",
                    view.status, view.terminal, view.retryable
                );
                if view.status.as_str() == "SUCCESS" {
                    println!("NOTE: provider SUCCESS 不等于 billing committed；以 envelope 的 committedAt 字段为准。");
                }
            }
            Err(e) => match e.kind {
                CompliancePollErrorKind::Timeout => {
                    eprintln!(
                        "[provider request] still pending —— DO NOT 重发原请求；下次查询/对账"
                    )
                }
                CompliancePollErrorKind::TerminalFailure => {
                    eprintln!("[provider request] FAILED —— 走人工对账")
                }
                _ => return Err(e.into()),
            },
        }
    }

    Ok(())
}
