//! compliance_read — Compliance 只读查询。
//!
//! 端口自 `acosmi-sdk-ts/examples/compliance-read.ts`。
//!
//! 演示：
//!   1. OAuth 登录（按业务最小集合申请 compliance scope）。
//!   2. 通过 evidence_no 做公开 verify（`verify_evidence_public`，匿名可调用；返回字段不含
//!      PII / 合同原文 / storage）。
//!   3. 查询已申请的时间章 / 已发布的报告 / 已创建的签署 envelope。
//!
//! 严禁：本示例 / SDK / 仓库不包含 provider endpoint、证书/密钥材料、口令、provider 原始报文、
//! callback billing commit 字段。
//!
//! 环境变量：
//!   - `ACOSMI_SERVER_URL`（必填）：网关 base URL。
//!   - `ACOSMI_COMPLIANCE_BASE_URL`（可选）：compliance 独立 ingress；缺省 `${server_url}/admin-api`。
//!   - `EVIDENCE_NO`（可选）：公开 verify 的 evidence_no，缺省 `EV-2026-0001`。
//!   - `TIMESTAMP_TOKEN_ID` / `REPORT_ID` / `ENVELOPE_ID`（可选）：读取详情的数字 id。
//!
//! 运行：`cargo run --example compliance_read`（CI 仅 `cargo build --example compliance_read`）。

use acosmi::{Client, Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_url = std::env::var("ACOSMI_SERVER_URL").expect("ACOSMI_SERVER_URL is required");
    let compliance_base_url = std::env::var("ACOSMI_COMPLIANCE_BASE_URL").ok();

    let client = Client::create(Config {
        server_url: Some(server_url),
        // compliance_base_url 不配置 → 默认 ${server_url}/admin-api。
        compliance_base_url,
        ..Default::default()
    })
    .await?;

    let scopes: Vec<String> = vec![
        "compliance:evidence:read".into(),
        "compliance:timestamp:verify".into(),
        "compliance:contract_signing:read".into(),
        "compliance:reports:read".into(),
    ];
    client
        .login("Compliance Read Example", &scopes, None)
        .await?;

    let compliance = client.compliance();

    // === 公开 verify ===
    // 通过对外稳定的 evidence_no 查询。该端点不要求 compliance scope（匿名可调用；
    // verify_evidence_public 内部：已持有 token 时附 Authorization 以保留审计上下文，否则匿名请求）。
    let evidence_no = std::env::var("EVIDENCE_NO").unwrap_or_else(|_| "EV-2026-0001".into());
    let verify_result = compliance
        .verify_evidence_public(Some(&evidence_no), None, None)
        .await?;
    println!(
        "[public verify] content_hash: {}",
        verify_result.content_hash
    );
    println!("[public verify] verified_at: {}", verify_result.verified_at);
    // 注意：以下字段不在返回中（隐私边界）：
    //   - storageBucket / storageKey / subjectSnapshotId
    //   - 用户手机号 / 邮箱 / 真实姓名
    //   - 合同原文 / provider 内部主体 id / TSA 内部 object id

    // === 读时间章 / 报告 / envelope ===
    let token_id: i64 = env_i64("TIMESTAMP_TOKEN_ID", 1);
    let token = compliance.get_timestamp(token_id, None).await?;
    println!(
        "[timestamp] {} serial_number={:?} status={}",
        token.id, token.serial_number, token.verification_status
    );

    let report_id: i64 = env_i64("REPORT_ID", 1);
    let report = compliance.get_report(report_id, None).await?;
    println!(
        "[report] {} {} status={}",
        report.id, report.report_no, report.status
    );

    // 租户由 access token principal 推导，SDK 不发送 tenant-id header。
    let envelope_id: i64 = env_i64("ENVELOPE_ID", 0);
    if envelope_id > 0 {
        let envelope = compliance.get_signing_envelope(envelope_id, None).await?;
        println!(
            "[envelope] {} status={} pending_reason={:?}",
            envelope.envelope_no, envelope.status, envelope.pending_reason
        );
    }

    Ok(())
}

fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}
