//! v1.6.0 业务侧终端用户 id（endUserId）公共工具。端口自 `models/enduser.ts`。
//!
//! 网关 sanitizer 仍会做权限校验 + 派生兜底；此处仅负责 SDK 侧的基本约束：
//!   - 正则：`[a-zA-Z0-9_-]+`
//!   - 长度：≤ 512

/// `MAX_END_USER_ID_LENGTH` 与上游 DeepSeek 官方文档对齐。
pub const MAX_END_USER_ID_LENGTH: usize = 512;

/// 检查 `end_user_id` 是否符合规范（字符集 + 长度）。
/// 空串/`None` 视为合法（表示未设置），返回 `None`。
///
/// 返回 `Some(错误信息)` 或 `None` 表示合法。
///
/// # Examples
///
/// ```
/// use acosmi::validate_end_user_id;
///
/// assert!(validate_end_user_id(Some("user-abc-123")).is_none()); // 合法
/// assert!(validate_end_user_id(Some("has space")).is_some());     // 非法字符
/// assert!(validate_end_user_id(None).is_none());                  // 未设置 = 合法
/// ```
pub fn validate_end_user_id(s: Option<&str>) -> Option<String> {
    let s = match s {
        Some(s) if !s.is_empty() => s,
        _ => return None,
    };
    // 长度按 UTF-16 code unit（对齐 TS string.length）。
    let len = s.encode_utf16().count();
    if len > MAX_END_USER_ID_LENGTH {
        return Some(format!("endUserId length {len} > {MAX_END_USER_ID_LENGTH}"));
    }
    for (offset, c) in s.char_indices() {
        let code = c as u32;
        let ok = (0x61..=0x7a).contains(&code) // a-z
            || (0x41..=0x5a).contains(&code) // A-Z
            || (0x30..=0x39).contains(&code) // 0-9
            || code == 0x5f // _
            || code == 0x2d; // -
        if !ok {
            return Some(format!(
                "endUserId contains invalid char code 0x{code:x} at offset {offset} (allowed: [a-zA-Z0-9_-])"
            ));
        }
    }
    None
}

/// 识别 SSE 协议中的注释行（`":<text>"`），如 `": keep-alive"`。
///
/// 严格定义：行的首字节是 `:`。不放宽允许行首空白（SSE 规范不允许）。
pub fn is_sse_comment_line(line: &str) -> bool {
    line.as_bytes().first() == Some(&b':')
}
