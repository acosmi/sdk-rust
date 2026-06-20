//! ManagedModel catalog helpers（v1.2+）。端口自 `models/model-helpers.ts`。
//!
//! 让 CrabCode / CrabClaw 在做 desktop automation / computer-use 选模型时完全依赖 SDK
//! `list_models` 下发的 capabilities + input_modalities 字段，杜绝任何基于模型名 substring
//! 的硬编码推断。
//!
//! 红线：
//!   1. 这里没有任何 model name match —— 只读 [`ManagedModel`] 字段。
//!   2. 缺失字段一律保守判负 —— 缺 input_modalities 视为 "未声明"，不当 image-capable。
//!   3. `supports_desktop_visual_understanding` 与 `inputModalities=['image']` 正交。

use super::types::{InputModality, ManagedModel};

/// 判断 [`ManagedModel`] 是否声明支持指定输入模态。
/// `None` model → false；`input_modalities` 缺失 → false（保守）。
pub fn model_supports_input_modality(
    model: Option<&ManagedModel>,
    modality: InputModality,
) -> bool {
    let model = match model {
        Some(m) => m,
        None => return false,
    };
    match &model.input_modalities {
        Some(mods) => mods.contains(&modality),
        None => false,
    }
}

/// 等价 `model_supports_input_modality(model, Image)`。
pub fn model_supports_image_input(model: Option<&ManagedModel>) -> bool {
    model_supports_input_modality(model, InputModality::Image)
}

/// 在 catalog 中按顺序查找首个支持指定模态的模型。
///
/// 选择规则：`is_enabled != false`（默认开）+ `input_modalities` 含指定模态 + catalog 顺序首个。
pub fn find_first_model_by_input_modality(
    models: &[ManagedModel],
    modality: InputModality,
) -> Option<ManagedModel> {
    for m in models {
        // Rust ManagedModel.is_enabled 是非可选 bool；TS `isEnabled === false` 显式剔除。
        if !m.is_enabled {
            continue;
        }
        if !model_supports_input_modality(Some(m), modality) {
            continue;
        }
        return Some(m.clone());
    }
    None
}

/// 在 catalog 中选出最适合做 "桌面视觉理解 sidecar" 的模型。
///
/// 选择规则（按用户指定顺序）：
///   1. `is_enabled != false`
///   2. `capabilities.supports_desktop_visual_understanding == true`
///   3. `input_modalities` 含 image（必须真能吃图）
///   4. 命中集合中优先 `is_default == true`；否则返回 catalog 顺序第一个
///
/// 全部不满足返 `None`。
pub fn find_desktop_visual_understanding_model(models: &[ManagedModel]) -> Option<ManagedModel> {
    let mut candidates: Vec<&ManagedModel> = Vec::new();
    for m in models {
        if !m.is_enabled {
            continue;
        }
        if m.capabilities.supports_desktop_visual_understanding != Some(true) {
            continue;
        }
        if !model_supports_input_modality(Some(m), InputModality::Image) {
            continue;
        }
        candidates.push(m);
    }
    if candidates.is_empty() {
        return None;
    }
    for m in &candidates {
        // 注：公开 list_models 不返回 is_default，此分支仅对 admin 上下文生效；
        // 公开数据下落到 catalog 顺序首个（可接受）。
        if m.is_default == Some(true) {
            return Some((*m).clone());
        }
    }
    Some(candidates[0].clone())
}
