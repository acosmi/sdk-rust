//! P6 结构闸门：`do_json_full_raw` 的每个调用点都必须**显式**声明自己的超时预算。
//!
//! 由来（2026-08-06）：`generate_video` 长期传 `DEFAULT_JSON_TIMEOUT_MS`(30s)，而它是生成
//! 端点。同一处错误在三个兄弟 SDK 的**同一个方法**上同时存在 —— TypeScript 的
//! `generateVideo` 漏传 `doJSONFullRaw` 的第 5 实参，Go 的 `GenerateVideo` 干脆一个 deadline
//! 都不设。三份独立实现犯同一个错，说明问题不在人而在形状：**控制面默认值对调用点是
//! 隐式的，谁都看不见自己继承了什么。**
//!
//! 同一轮还查出第二种形态：`/skill-generator/*` 三条路由服务端用的是 120s 的 LLM 客户端，
//! 客户端却经 `billing_post_body` 封在 30s —— 内外预算倒挂，客户端必先死。
//!
//! 所以闸门是 **default-deny**：用控制面默认预算必须进具名 allowlist 并写明理由。
//!
//! 刻意扫**整个 `src/`** 而不是单个文件：本闸门的第一版只扫 `core/client.rs`，当场漏掉了
//! `billing/mod.rs` 里的同族调用点（14 个调用点只照到 9 个）。一个有盲区的闸门会给出
//! 「已覆盖」的错觉，比没有闸门更糟。

use std::fs;
use std::path::{Path, PathBuf};

/// 允许在调用点使用控制面默认预算的方法，及其理由。
const DEFAULT_TIMEOUT_ALLOWLIST: &[(&str, &str)] = &[
    ("do_json_full", "控制面通用包装器 —— 所有非推理端点走它"),
    (
        "poll_video_task",
        "视频任务状态轮询 —— 控制面 GET，不承载推理",
    ),
    (
        "billing_post_body",
        "计费域通用包装器 —— 订单 / 额度等控制面写操作；生成类调用走 _with_timeout 变体",
    ),
    (
        "notify_void",
        "通知域控制面 POST（标记已读 / 订阅等空返回写操作），不承载推理",
    ),
    (
        "ws_connect_once",
        "WebSocket 连接票据换取 —— 控制面握手，不承载推理",
    ),
];

/// 推理 / 生成端点的路径特征。凡请求这些路径的方法，都必须在本方法体内出现具名的
/// 生成预算 —— 光靠扫 `do_json_full_raw` 的实参抓不到「生成端点借道控制面包装器出去」
/// 这一形态（`/skill-generator/*` 经 `billing_post_body` 就是这么漏掉的：包装器本身在
/// allowlist 里，于是调用方一路绿灯）。
const INFERENCE_PATH_MARKERS: &[&str] = &[
    "/skill-generator/",
    "/videos/generations",
    "/images/generations",
    "/embeddings",
    "/rerank",
];

fn src_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("读取 src 目录") {
        let path = entry.expect("目录项").path();
        if path.is_dir() {
            src_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 截到第一个 `#[cfg(test)]` **模块**为止，只审生产代码。
///
/// 必须按「cfg(test) 模块」切而不是按 `#[test]` 属性判：测试模块里的 helper 不带属性，
/// 位置型判据在交错分布的文件上必错。实测本仓的 `core/client.rs` 里 `#[cfg(test)]` 还
/// 标在几个 impl 内的测试专用方法上（`prime_tokens_for_test` 等）—— 那些不能当切点，
/// 否则会把它们后面的生产代码一并切掉。Rust 惯例把测试模块放在文件末尾，故取第一个
/// cfg(test) **模块**为界；扫描器一旦因此少扫，下面的调用点下限断言会当场红。
fn production_region(src: &str) -> String {
    let normalized = src.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        let next = lines[i + 1..].iter().find(|l| !l.trim().is_empty());
        if let Some(n) = next {
            let t = n.trim_start();
            if t.starts_with("mod ") || t.starts_with("pub mod ") {
                return lines[..i].join("\n");
            }
        }
    }
    normalized
}

fn fn_name_of_line(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let t = t
        .strip_prefix("pub(crate) ")
        .or_else(|| t.strip_prefix("pub "))
        .unwrap_or(t);
    let t = t.strip_prefix("async ").unwrap_or(t);
    let t = t.strip_prefix("fn ")?;
    let name = t
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("");
    (!name.is_empty()).then_some(name)
}

/// 从调用点起做括号配平取出整段实参文本。刻意不逐字比对代码块 —— 那种断言会因为任何
/// 无关的换行或重命名假红。
fn balanced_args(src: &str, start: usize) -> &str {
    let open = start + src[start..].find('(').expect("调用点必有左括号");
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    for i in open..bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &src[open..=i];
                }
            }
            _ => {}
        }
    }
    panic!("调用括号未配平");
}

/// 返回 (调用点数, 违规描述)。抽成纯函数是为了能用合成源码做负向对照 —— 扫描器自己坏掉
/// 时闸门会恒绿，那比没有闸门更糟。
fn audit(src: &str, origin: &str) -> (usize, Vec<String>) {
    // 运行时拼 needle，避免本文件源码里出现同样的连续字面量造成自匹配。
    let needle = format!(".do_json{}", "_full_raw(");
    let mut current_fn = String::new();
    let mut offset = 0usize;
    let mut seen = 0usize;
    let mut bad = Vec::new();
    // 行尾归一后再切：本仓是 CRLF，按「行长 + 1」累加字节偏移会逐行漂移（这个坑本轮在
    // TS 侧的同族闸门上已经踩过一次，当场造出 5 个假阳性）。
    let normalized = src.replace("\r\n", "\n");
    for line in normalized.split('\n') {
        if let Some(name) = fn_name_of_line(line) {
            current_fn = name.to_string();
        }
        if let Some(rel) = line.find(&needle) {
            seen += 1;
            let args = balanced_args(&normalized, offset + rel);
            let uses_default = args.contains("DEFAULT_JSON_TIMEOUT_MS");
            let uses_chat = args.contains("CHAT_REQUEST_TIMEOUT_MS");
            let uses_param = args.contains("timeout_ms");
            if !uses_default && !uses_chat && !uses_param {
                bad.push(format!("{origin}::{current_fn} 未声明具名超时预算"));
            } else if uses_default
                && !DEFAULT_TIMEOUT_ALLOWLIST
                    .iter()
                    .any(|(n, _)| *n == current_fn)
            {
                bad.push(format!(
                    "{origin}::{current_fn} 使用控制面默认预算 DEFAULT_JSON_TIMEOUT_MS；推理/生成端点必须显式传 CHAT_REQUEST_TIMEOUT_MS，确属控制面请连同理由加入 DEFAULT_TIMEOUT_ALLOWLIST"
                ));
            }
        }
        offset += line.len() + 1;
    }
    (seen, bad)
}

#[test]
fn every_do_json_full_raw_call_declares_its_budget() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    src_files(&root, &mut files);
    assert!(
        files.len() > 20,
        "只找到 {} 个源文件，疑似扫描器失效",
        files.len()
    );

    let mut total = 0usize;
    let mut bad = Vec::new();
    for f in &files {
        let text = fs::read_to_string(f).expect("读取源文件");
        let origin = f
            .strip_prefix(&root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        let (seen, mut v) = audit(&production_region(&text), &origin);
        total += seen;
        bad.append(&mut v);
    }

    assert!(
        bad.is_empty(),
        "超时预算闸门发现 {} 处违规：\n{}",
        bad.len(),
        bad.join("\n")
    );
    // 扫描器坏掉（needle 改名、目录结构变化）会让它一个都找不到从而恒绿。
    assert!(
        total >= 12,
        "只扫到 {total} 个调用点，疑似扫描器失效（应 >= 12）"
    );
}

/// 按**端点路径**判定：请求推理 / 生成路径的方法体内必须出现具名生成预算。
///
/// 这道闸门与上一道互补，缺了它就会漏掉「生成端点借道控制面包装器」的形态。
fn audit_by_endpoint(src: &str, origin: &str) -> (usize, Vec<String>) {
    let normalized = src.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut fn_starts: Vec<(usize, String)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(name) = fn_name_of_line(line) {
            fn_starts.push((i, name.to_string()));
        }
    }
    let mut seen = 0usize;
    let mut bad = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !INFERENCE_PATH_MARKERS.iter().any(|m| line.contains(m)) {
            continue;
        }
        // 只看字符串字面量里的路径，跳过文档注释。
        if line.trim_start().starts_with("//") || !line.contains('"') {
            continue;
        }
        seen += 1;
        let Some(pos) = fn_starts.iter().rposition(|(start, _)| *start <= i) else {
            continue;
        };
        let (start, name) = &fn_starts[pos];
        let end = fn_starts
            .get(pos + 1)
            .map(|(s, _)| *s)
            .unwrap_or(lines.len());
        let body = lines[*start..end].join("\n");
        if !body.contains("CHAT_REQUEST_TIMEOUT_MS") {
            bad.push(format!(
                "{origin}::{name} 请求推理/生成端点却未在方法体内声明生成预算 CHAT_REQUEST_TIMEOUT_MS —— 借道控制面包装器不算声明"
            ));
        }
    }
    (seen, bad)
}

#[test]
fn every_inference_endpoint_declares_a_generation_budget() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    src_files(&root, &mut files);

    let mut total = 0usize;
    let mut bad = Vec::new();
    for f in &files {
        let text = fs::read_to_string(f).expect("读取源文件");
        let origin = f
            .strip_prefix(&root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        let (seen, mut v) = audit_by_endpoint(&production_region(&text), &origin);
        total += seen;
        bad.append(&mut v);
    }

    assert!(
        bad.is_empty(),
        "端点闸门发现 {} 处违规：\n{}",
        bad.len(),
        bad.join("\n")
    );
    assert!(
        total >= 5,
        "只扫到 {total} 处推理端点路径，疑似扫描器失效（应 >= 5）"
    );
}

#[test]
fn endpoint_gate_catches_generation_routed_through_a_control_plane_wrapper() {
    // 负向对照：这正是 `/skill-generator/*` 曾经的形状 —— 包装器在 allowlist 里，
    // 于是第一道闸门一路绿灯，而调用方其实继承了 30s。
    let synthetic = concat!(
        "    pub async fn generate_skill(\n",
        "        &self,\n",
        "    ) -> Result<()> {\n",
        "        self.billing_post_body(\"/skill-generator/generate\", Some(&body), signal)\n",
        "            .await\n",
        "    }\n",
    );
    let (seen, bad) = audit_by_endpoint(synthetic, "synthetic.rs");
    assert_eq!(seen, 1, "合成源码应恰有 1 处推理端点路径");
    assert_eq!(bad.len(), 1, "闸门应报出 1 处违规，实际 {bad:?}");
    assert!(bad[0].contains("generate_skill"), "应点名方法：{}", bad[0]);
}

#[test]
fn gate_catches_a_generation_endpoint_that_inherits_the_default() {
    // 负向对照：合成一个「新增生成端点忘了传预算」的源码，闸门必须点名它。
    let synthetic = concat!(
        "    pub async fn generate_music(\n",
        "        &self,\n",
        "    ) -> Result<()> {\n",
        "        let x = self\n",
        "            .do_json_full_raw(\n",
        "                reqwest::Method::POST,\n",
        "                &endpoint,\n",
        "                Some(&body),\n",
        "                signal,\n",
        "                DEFAULT_JSON_TIMEOUT_MS,\n",
        "            )\n",
        "            .await?;\n",
        "    }\n",
    );
    let (seen, bad) = audit(synthetic, "synthetic.rs");
    assert_eq!(seen, 1, "合成源码应恰有 1 个调用点");
    assert_eq!(bad.len(), 1, "闸门应报出 1 处违规，实际 {bad:?}");
    assert!(
        bad[0].contains("generate_music"),
        "违规描述应点名方法：{}",
        bad[0]
    );
}

#[test]
fn gate_accepts_an_allowlisted_control_plane_call() {
    let synthetic = concat!(
        "    pub async fn poll_video_task(\n",
        "        .do_json_full_raw(m, p, b, s, DEFAULT_JSON_TIMEOUT_MS)\n",
    );
    let (seen, bad) = audit(synthetic, "synthetic.rs");
    assert_eq!(seen, 1);
    assert!(
        bad.is_empty(),
        "allowlist 内的控制面调用不该被判违规：{bad:?}"
    );
}

#[test]
fn gate_accepts_a_call_that_forwards_a_timeout_parameter() {
    // 转发形参的包装器（如 `_with_timeout` 变体）由其调用方负责选值，本层不判。
    let synthetic = concat!(
        "    pub(crate) async fn billing_post_body_with_timeout(\n",
        "        .do_json_full_raw(m, p, b, s, timeout_ms)\n",
    );
    let (seen, bad) = audit(synthetic, "synthetic.rs");
    assert_eq!(seen, 1);
    assert!(bad.is_empty(), "转发形参的包装器不该被判违规：{bad:?}");
}
