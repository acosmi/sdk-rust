//! sanitize/config — 端口自 `acosmi-sdk-ts/src/sanitize/config.ts`
//! （其本身端口自 `acosmi-sdk-go/sanitize/config.go`）。
//!
//! SDK 层防御性配置 + 哨兵错误。TS 侧的 `ErrHistoryTooDeep` / `ErrBlockDenied` 单例 +
//! `SizeError` class（`instanceof` 区分）→ 按 §5 P8 验收清单①收敛为
//! [`SanitizeError`] enum 变体 + `match`（非单例相等）。

use super::types::BlockType;

/// SDK 层防御性配置。只做所有下游 provider 都不可能接受的底线剥除；具体 provider 适配由网关承担。
///
/// 零值 / `None` = 不校验 / 不剥除，等价于未启用（对齐 TS `?` 字段缺省语义）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MinimalSanitizeConfig {
    /// 内联 base64 image 体积上限（字节）。`None`/`0` = 不校验。URL 版交网关把关。
    pub max_image_bytes: Option<u64>,
    /// 内联 base64 video 体积上限（字节）。`None`/`0` = 不校验。
    pub max_video_bytes: Option<u64>,
    /// 内联 base64 document(PDF) 体积上限（字节）。`None`/`0` = 不校验。
    pub max_pdf_bytes: Option<u64>,

    /// history 轮次硬上限（防止内存爆炸 / 上行带宽）。`None`/`0` = 不校验。
    /// 超限 → [`SanitizeError::HistoryTooDeep`]。
    pub max_messages_turns: Option<u64>,

    /// 公共黑名单（所有 provider 均拒绝的 block 类型）。默认空。
    pub permanent_deny_blocks: Vec<BlockType>,
}

impl MinimalSanitizeConfig {
    /// 是否完全未配置（所有字段为零值）。对齐 sanitize-bridge `cfg == null` 的零开销判定，
    /// 用于 [`crate::core::Client::set_defensive_sanitize`] 传空 `{}` 等价关闭的语义。
    pub fn is_empty(&self) -> bool {
        self.max_image_bytes.unwrap_or(0) == 0
            && self.max_video_bytes.unwrap_or(0) == 0
            && self.max_pdf_bytes.unwrap_or(0) == 0
            && self.max_messages_turns.unwrap_or(0) == 0
            && self.permanent_deny_blocks.is_empty()
    }
}

/// sanitize 域哨兵错误。对齐 TS `HistoryTooDeepError` / `BlockDeniedError` / `SizeError`
/// 三类（§5 P8 验收①：单例 → enum 变体 + `match`，非 `instanceof` 相等）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SanitizeError {
    /// 历史轮次超过配置深度。对应 TS 哨兵 `ErrHistoryTooDeep`。
    #[error("sanitize: messages history exceeds configured depth")]
    HistoryTooDeep,

    /// block 类型被永久 deny-list 拒绝。对应 TS 哨兵 `ErrBlockDenied`。
    ///
    /// （TS 侧定义了该哨兵但当前剥除路径走静默剥除而非抛错；保留变体作跨语言锚点。）
    #[error("sanitize: block type permanently denied")]
    BlockDenied,

    /// 内联 base64 媒体体积超限。携带 block 类型 + 实际/上限字节数（对应 TS `SizeError`）。
    #[error("sanitize: {block_type} base64 size {actual} exceeds limit {limit}")]
    Size {
        block_type: BlockType,
        actual: u64,
        limit: u64,
    },
}
