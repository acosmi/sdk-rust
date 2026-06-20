//! compliance_timestamp — hash-only evidence + 时间章链路。
//!
//! 端口自 `acosmi-sdk-ts/examples/compliance-evidence-timestamp.ts`。
//!
//! 演示：
//!   1. 本地对业务内容做 sha256（不传原文）。
//!   2. 创建 hash-only evidence asset（`create_evidence_asset`）。
//!   3. 给资产申请时间章（`issue_timestamp_for_asset`，持久化 Idempotency-Key）。
//!   4. polling 等到本地 verify 通过（`wait_for_timestamp_verified`）。
//!   5. 构建 evidence package（`build_evidence_package`）。
//!   6. 创建报告并下载离线复核 VO（`create_report` + `download_report`）。
//!
//! 红线：
//!   - 不传 provider 字段；服务端按配置选 provider。
//!   - 不读取证书 / 密钥材料或 provider 签名材料；这些是后端实现细节。
//!   - Idempotency-Key 必须在内存外持久化，重启 / 重试时复用同一 key。
//!
//! 环境变量：
//!   - `ACOSMI_SERVER_URL`（必填）：网关 base URL。
//!   - `ACOSMI_COMPLIANCE_BASE_URL`（可选）：compliance 独立 ingress；缺省 `${server_url}/admin-api`。
//!
//! 运行：`cargo run --example compliance_timestamp`（CI 仅 `cargo build --example compliance_timestamp`）。

use std::collections::HashMap;

use acosmi::compliance::{ComplianceAssetType, ComplianceDigestSource, CompliancePrivacyLevel};
use acosmi::{
    Client, CompliancePollOptions, ComplianceWriteOptions, Config, CreateEvidenceAssetRequest,
    CreateReportRequest,
};
use sha2::{Digest, Sha256};

// 模拟持久化的 Idempotency-Key 存储；生产环境应落 DB / 本地文件 / 业务订单表。
fn load_or_create_key(store: &mut HashMap<String, String>, slot: &str) -> String {
    store
        .entry(slot.to_string())
        .or_insert_with(|| {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            format!("{slot}-{nonce}")
        })
        .clone()
}

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

    // 按业务最小集合申请 compliance scope。
    let scopes: Vec<String> = vec![
        "compliance:evidence:read".into(),
        "compliance:evidence:write".into(),
        "compliance:timestamp:issue".into(),
        "compliance:timestamp:verify".into(),
        "compliance:reports:read".into(),
        "compliance:reports:write".into(),
    ];
    client
        .login("Evidence + Timestamp Example", &scopes, None)
        .await?;

    let compliance = client.compliance();
    let mut key_store: HashMap<String, String> = HashMap::new();

    // 1) 本地 sha256（用户业务内容）。
    let content = b"release v1.2.3 manifest line-1\nrelease v1.2.3 manifest line-2";
    let sha256_hex = hex_lower(&Sha256::digest(content));

    // 2) 创建 hash-only evidence asset。
    let asset_key = load_or_create_key(&mut key_store, "asset:release-v1.2.3");
    let asset = compliance
        .create_evidence_asset(
            &CreateEvidenceAssetRequest {
                asset_type: ComplianceAssetType::from(ComplianceAssetType::HASH_ONLY),
                name: "release-v1.2.3.manifest".into(),
                hash_algorithm: acosmi::compliance::ComplianceHashAlgorithm::from(
                    acosmi::compliance::ComplianceHashAlgorithm::SHA256,
                ),
                declared_hash: Some(sha256_hex),
                digest_source: Some(ComplianceDigestSource::from(ComplianceDigestSource::CLIENT)),
                privacy_level: Some(CompliancePrivacyLevel::from(
                    CompliancePrivacyLevel::PRIVATE,
                )),
                ..Default::default()
            },
            &ComplianceWriteOptions {
                idempotency_key: Some(asset_key),
            },
            None,
        )
        .await?;
    println!("[asset] {} {}", asset.id, asset.evidence_no);

    // 3) 申请时间章 —— Idempotency-Key 持久化复用。
    let ts_key = load_or_create_key(&mut key_store, "ts:release-v1.2.3");
    let token = compliance
        .issue_timestamp_for_asset(
            asset.id,
            &ComplianceWriteOptions {
                idempotency_key: Some(ts_key),
            },
            None,
        )
        .await?;
    println!(
        "[timestamp issued] {} status={}",
        token.id, token.verification_status
    );

    // 4) polling 到本地 verify 通过。
    let poll_opts = CompliancePollOptions {
        timeout_ms: Some(60_000),
        initial_interval_ms: Some(1_000),
        max_interval_ms: Some(5_000),
        ..Default::default()
    };
    match compliance
        .wait_for_timestamp_verified(token.id, &poll_opts, None)
        .await
    {
        Ok(verified) => println!(
            "[timestamp verified] serial={:?} gen_time={:?}",
            verified.serial_number, verified.gen_time
        ),
        Err(e) => {
            use acosmi::CompliancePollErrorKind as K;
            match e.kind {
                K::TerminalFailure => {
                    eprintln!(
                        "timestamp local verify failed —— DO NOT retry with same key; 起新链路"
                    );
                    return Err(e.into());
                }
                K::Timeout => {
                    eprintln!("timestamp still UNKNOWN; polling timed out —— wait for sync or retry later");
                    return Ok(()); // 不自动重发原 provider 请求。
                }
                _ => return Err(e.into()),
            }
        }
    }

    // 5) 构建 evidence package。
    let pkg_key = load_or_create_key(&mut key_store, "pkg:release-v1.2.3");
    let pkg = compliance
        .build_evidence_package(
            asset.id,
            Some(token.id),
            &ComplianceWriteOptions {
                idempotency_key: Some(pkg_key),
            },
            None,
        )
        .await?;
    println!("[package] {}", pkg.id);

    // 6) 创建报告并下载离线复核 VO。
    let report_key = load_or_create_key(&mut key_store, "report:release-v1.2.3");
    let report = compliance
        .create_report(
            &CreateReportRequest {
                asset_id: asset.id,
                package_id: pkg.id,
            },
            &ComplianceWriteOptions {
                idempotency_key: Some(report_key),
            },
            None,
        )
        .await?;
    let download = compliance.download_report(report.id, None).await?;
    println!(
        "[report download] {} asset_content_hash={:?} ts_serial={:?}",
        download.report_no, download.asset_content_hash, download.timestamp_serial_number
    );

    Ok(())
}

/// bytes → 小写 hex 字符串。
fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
