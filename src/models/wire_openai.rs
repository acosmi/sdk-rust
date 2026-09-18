//! OpenAI 兼容 wire-format 响应 DTO（非 Anthropic 厂商）。端口自 `models/wire-openai.ts`
//! （其端口自 `acosmi-sdk-go/types.go` v0.19.0 的 OpenAI 兼容响应类型段）。
//!
//! 命名约定：字段名 = Go json tag 字面量（wire format），不做 camelCase 重映射。

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// 反序列化 `string | null` 为 `String`：`null` 落成空串。配合 `#[serde(default)]` 时键缺席同样
/// 落成空串，于是「缺席」「`null`」「空串」在下游三者同形。
///
/// 参照 TS `models/adapters/openai.ts`：那边读这些字段一律走 truthiness
/// （`if (choice.message.content && choice.message.content !== '')`），`null` / `undefined` / `''`
/// 走同一条分支。Rust 侧字段类型保持 `String`（不改公开签名、序列化形态不变），只把反序列化的
/// 宽容度对齐过去。
fn de_string_or_empty<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// 反序列化 `arguments`：按 OpenAI 规范是 JSON 字符串，但部分兼容实现直接给对象/数组/数字。
/// 非字符串一律用其 JSON 文本形态落成串（等价 TS 的 `JSON.stringify(fn.arguments)`），
/// `null` 落成空串（等价 TS 的 `fn.arguments !== null` 守卫：不发 delta）。
///
/// 字段类型仍是 `String`，`Serialize` 形态不变。
fn de_arguments_as_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::Null => String::new(),
        Value::String(s) => s,
        other => other.to_string(),
    })
}

/// 反序列化非流式 `usage`：`null` 落成三计数皆 0 的默认值。配合 `#[serde(default)]` 时键缺席同样
/// 落成默认值，于是「缺席」与「`null`」在下游同形 —— 两者说的是同一件事：上游没有用量数据。
///
/// 参照 TS：`JSON.parse` 之后 `oai.usage` 为 `null` / `undefined` 时读 `oai.usage.prompt_tokens`
/// 会抛，但 `convertOpenAIToChatResponse` 读的是可选链后的值，两种形态都不让整个响应失败。
fn de_usage_or_default<'de, D>(deserializer: D) -> std::result::Result<OpenAIUsage, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<OpenAIUsage>::deserialize(deserializer)?.unwrap_or_default())
}

/// 反序列化**非流式** `choices`：`null` 落成空 `Vec`。配合 `#[serde(default)]` 时键缺席同样落成空
/// `Vec`，于是「缺席」与「`null`」在下游同形 —— 两者说的是同一件事：上游没有 choices。
///
/// 与流式那侧的 [`de_choices_lenient`] **刻意不是同一个函数**：元素类型不同（[`OpenAIChatChoice`]
/// vs [`OpenAIStreamChoice`]），为复用把两个 wire 类型合并就是在制造下一个 bug。宽容度也刻意窄一档
/// —— 只收 `null`，对象 / 标量仍然报错：一帧异形的流式 chunk 只该让那一帧零事件（流还要继续），
/// 而整个非流式响应体的 `choices` 不是数组，是这次调用彻底坏掉，不该被悄悄抹成「没有内容」。
/// 这一档与 Go 的 `[]OpenAIChatChoice` 相同（`json.Unmarshal` 收 `null`、拒对象）。
fn de_choices_or_empty<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<OpenAIChatChoice>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<OpenAIChatChoice>>::deserialize(deserializer)?.unwrap_or_default())
}

/// 反序列化流式 `choices`：不是数组（对象 / `null` / 标量）时落成空 `Vec` 而不是报错。
///
/// 等价 TS `OpenAIStreamConverter.convert` 的 `if (!Array.isArray(chunk.choices) …) return`：
/// 病态上游或网关异形帧只该让这一帧零事件，不该让整条流终止。数组内元素本身形态不对仍然报错
/// ——那是「有 choices 但解不动」，与「压根不是 choices 数组」是两回事。
fn de_choices_lenient<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<OpenAIStreamChoice>, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        value @ Value::Array(_) => serde_json::from_value(value).map_err(serde::de::Error::custom),
        _ => Ok(Vec::new()),
    }
}

/// OpenAI 兼容同步响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIChatResponse {
    pub id: String,
    /// "chat.completion"
    pub object: String,
    pub model: String,
    /// 缺席**或显式 `null`** 落成空 `Vec`：零内容块，`id` / `model` / `usage` 照常转换。此前必填且
    /// 非 null，两种形态分别以 `missing field \`choices\`` 与 `invalid type: null, expected a
    /// sequence` 让整个非流式响应解码失败（`parse_response` / `parse_openai_response_to_anthropic` /
    /// 直接 `from_str` 三个入口同时失败）—— 正文再完整，调用方一个字节也拿不到。
    ///
    /// 缺席那一半不是 Rust 比 TS 严：TS 此前在 `oai.choices.length` 处同样炸
    /// （`Cannot read properties of undefined`）；`null` 那一半 Rust 曾是三份里唯一的异类
    /// —— TS 的 `Array.isArray(oai.choices)` 守卫与 Go 的 `[]OpenAIChatChoice`（`json.Unmarshal`
    /// 把 `null` 解成 nil slice、`len(nil) == 0`）都容忍。2026-09-17 三份一并对齐到「容忍」：
    /// 消除异类就是这个立项存在的理由，留着它等于亲手制造一条新的三份分歧。
    ///
    /// 宽容度到 `null` 为止：对象 / 标量形态的 `choices` 仍然报错，理由见 [`de_choices_or_empty`]。
    #[serde(default, deserialize_with = "de_choices_or_empty")]
    pub choices: Vec<OpenAIChatChoice>,
    /// 缺席**或显式 `null`** 时三个计数都落成 0。此前必填且非 null，不结算用量的兼容实现（以及被
    /// 上游省略 `usage` 的错误回包）会以 `missing field \`usage\`` / `invalid type: null, expected
    /// struct OpenAIUsage` 让**整个非流式响应**解码失败 —— 正文明明在，调用方却什么都拿不到。
    /// TS 那边 `JSON.parse` 后读 `oai.usage.prompt_tokens` 得 `undefined`，两种形态都不会失败。
    ///
    /// 「缺席 / `null`」与「usage 在场但计数是 0」在本字段上同形：[`OpenAIUsage`] 的计数是 `i64`
    /// 不是 `Option<i64>`，这一点不在本次改动面内（流式路径另有 [`OpenAIStreamUsage`]）。
    #[serde(default, deserialize_with = "de_usage_or_default")]
    pub usage: OpenAIUsage,
}

/// OpenAI choices 元素。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIChatChoice {
    pub index: i64,
    pub message: OpenAIChatMessage,
    /// "stop", "tool_calls", "length"。
    ///
    /// nullable：被截断 / 未结算的回包上是 `null`。此前必填且非 null，那类回包在反序列化这一步就
    /// 报 `invalid type: null, expected a string`，整个非流式响应失败。TS 的 `switch` 默认臂
    /// （`resp.stop_reason = choice.finish_reason`）原样赋值、不抛错。
    #[serde(default, deserialize_with = "de_string_or_empty")]
    pub finish_reason: String,
}

/// OpenAI message。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIChatMessage {
    pub role: String,
    /// nullable，且可缺席：OpenAI 规范对「只有工具调用」的响应返回 `"content": null`，部分兼容
    /// 实现干脆不写这个键。此前必填且非 null，**标准的工具调用回包在反序列化这一步就失败**
    /// （`invalid type: null, expected a string` / `missing field \`content\``）。
    ///
    /// 三种形态统一落成空串，与同文件转换器的 `if !content.is_empty()` 一起复刻 TS 的
    /// truthiness 守卫：没有正文就不产 text 块。
    #[serde(default, deserialize_with = "de_string_or_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    /// GLM/DeepSeek thinking。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

/// OpenAI tool_call。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    /// "function"
    pub r#type: String,
    pub function: OpenAIFunctionCall,
}

/// OpenAI function call。
///
/// 两个字段在**流式**续片上都可能缺席：OpenAI 规范只在工具调用的首片给 `name`，后续增量只带
/// `index` + `arguments` 分片。此前两者必填，每一次 OpenAI 线的工具调用都在第二帧
/// `missing field \`name\`` 直接终止整条流。参照 TS 读的是 `fn?.name` / `fn?.arguments`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIFunctionCall {
    /// `Option<String>` 而不是「缺席落空串」：空串与缺席必须可分辨。转换器据此决定
    /// `content_block.name` 这个键**写不写**（TS `JSON.stringify` 对 `undefined` 省略该键），
    /// 落成 `""` 会让下游的「有 name 键」判据收到一个空名字。`null` 与缺席同落 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "de_arguments_as_string")]
    pub arguments: String,
}

/// OpenAI token 用量（**非流式**响应）。三个计数都必填。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

/// **流式** chunk 上的 usage 对象。与非流式的 [`OpenAIUsage`] 分开声明：流式帧上各计数都可能缺席
/// （线上常见 `{"prompt_tokens":1,"completion_tokens":2}` 而无 `total_tokens`），还带明细子对象。
/// 沿用 [`OpenAIUsage`] 等于把「三个计数一定在且是整数」的承诺强加给流式帧 —— 直接按公开类型
/// [`OpenAIStreamChunk`] 解帧的库用户会撞上 `missing field \`total_tokens\``。
///
/// 与 TS `models/wire-openai.ts::OpenAIStreamUsage` 逐字对齐。SDK 只搬 `prompt_tokens` /
/// `completion_tokens`（见 `models/adapters/openai.rs::read_stream_usage`）；明细字段仅作类型声明，
/// **不映射、不做净额换算** —— usage 的语义归一只在网关。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<i64>,
    /// 上游输入明细（如 `cached_tokens`）；SDK 不读取。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<OpenAIStreamUsagePromptDetails>,
    /// 上游输出明细（如 `reasoning_tokens`）；SDK 不读取。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<OpenAIStreamUsageCompletionDetails>,
}

/// [`OpenAIStreamUsage::prompt_tokens_details`] 的形状。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamUsagePromptDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<i64>,
}

/// [`OpenAIStreamUsage::completion_tokens_details`] 的形状。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamUsageCompletionDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
}

/// OpenAI SSE delta 格式。
///
/// `id` / `object` / `choices` 反序列化时可缺省（缺省即空值），序列化形态不变。线上确实存在没有
/// 这些字段的 data 帧：网关的错误契约帧，以及部分兼容实现的 usage-only 尾帧（证据见 TS
/// `models/wire-openai.ts` 同名类型 `choices` 字段的注释）。此前三者都是必填，这类帧在反序列化
/// 这一步就报错，整条流随之失败。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamChunk {
    #[serde(default)]
    pub id: String,
    /// "chat.completion.chunk"
    #[serde(default)]
    pub object: String,
    /// 非数组形态（对象 / `null` / 标量）落成空 `Vec`，见 [`de_choices_lenient`]。
    #[serde(default, deserialize_with = "de_choices_lenient")]
    pub choices: Vec<OpenAIStreamChoice>,
    /// 按 `stream_options.include_usage` 的帧序，非尾帧上为 null 或缺失，带值的是 `[DONE]` 之前的
    /// 尾帧 `{"choices":[],"usage":{...}}`。形状见 [`OpenAIStreamUsage`]（各计数皆可缺席）。
    ///
    /// 流式转换器仍不经本字段读 usage：它按原始 `Value` 读（见
    /// `models/adapters/openai.rs::read_stream_usage`），以免任何类型化约束决定整帧能否解析。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIStreamUsage>,
}

/// OpenAI SSE choice。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamChoice {
    /// 兼容实现常省略。转换器从不读它（只取 `choices[0]`），缺席按 0 处理即可；此前必填，
    /// 缺它的帧会以 `missing field \`index\`` 终止整条流。
    #[serde(default)]
    pub index: i64,
    /// 缺席落成「各字段皆 `None`」的空 delta：该 choice 零事件、不报错、不终止流。此前必填，
    /// 缺它的帧以 `missing field \`delta\`` 终止整条流。
    ///
    /// 这不是 Rust 比 TS 严：TS 在 `choice.delta.reasoning_content` 处同样炸（`undefined is not
    /// an object`），Go 反而给零值。三份实现在 2026-09-17 一并对齐到「容忍」。
    #[serde(default)]
    pub delta: OpenAIStreamDelta,
    /// nullable（`string | null`）。
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// OpenAI SSE delta。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIStreamToolCall>>,
}

/// OpenAI SSE tool_call delta。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIStreamToolCall {
    /// OpenAI 流式规范里必填，但兼容实现常省略。`i64` 时「省略」与「`index: 0`」同形，两个都不带
    /// index 的 tool_call 于是共用键 0 —— 只开一个块、两段参数拼进同一条 `partial_json` 流，产出
    /// `{…}{…}` 这种必然非法的 JSON，第二个调用彻底丢失（Go 侧实测正是这个形态）。
    ///
    /// 缺席时归属由 `models/adapters/openai.rs` 的 `resolve_tool_key` 按 index → `id:` → 沿用上一个
    /// 三级降级决定（对齐 TS `resolveToolKey`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// OpenAI 流式规范允许后续增量只带 `{index}` 而不带 `function`。缺席落成两字段皆空串的
    /// 默认值，于是既不建块也不发 delta；此前必填，这类空心增量会以 `missing field \`function\``
    /// 终止整条流。参照 TS 的 `const fn = tc.function as {…} | undefined`。
    #[serde(default)]
    pub function: OpenAIFunctionCall,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 序列化形态是 wire 契约的一半，必须被钉住、只在 major 版本上有意变更
    /// （`docs/开发与发布手册.md`：wire-format 不兼容变更 = major）。
    ///
    /// 5.0.0 有意改了两处：`OpenAIStreamToolCall.index` 与 `OpenAIFunctionCall.name` 变
    /// `Option` + `skip_serializing_if`，于是「上游没给」不再被写成 `0` / `""`。其余字段一字不动 ——
    /// 这条断言同时守住「改了的两个」和「没改的一批」：任何一处再动都必红。
    #[test]
    fn serialized_shape_is_pinned() {
        let message = OpenAIChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            tool_calls: None,
            reasoning_content: None,
        };
        // 非流式 message 的空 `content` 仍然照写（它必填、语义是「空正文」而非「没给」）。
        assert_eq!(
            serde_json::to_string(&message).unwrap_or_default(),
            r#"{"role":"assistant","content":""}"#
        );

        // 5.0.0：`name` 为 `None` ⇒ 省略该键（此前写 `"name":""`）。`arguments` 仍是 `String`。
        let function = OpenAIFunctionCall::default();
        assert_eq!(
            serde_json::to_string(&function).unwrap_or_default(),
            r#"{"arguments":""}"#
        );
        assert_eq!(
            serde_json::to_string(&OpenAIFunctionCall {
                name: Some("get_weather".to_string()),
                arguments: "{}".to_string(),
            })
            .unwrap_or_default(),
            r#"{"name":"get_weather","arguments":"{}"}"#
        );

        let choice = OpenAIChatChoice::default();
        assert_eq!(
            serde_json::to_string(&choice).unwrap_or_default(),
            r#"{"index":0,"message":{"role":"","content":""},"finish_reason":""}"#
        );

        let chunk = OpenAIStreamChunk::default();
        assert_eq!(
            serde_json::to_string(&chunk).unwrap_or_default(),
            r#"{"id":"","object":"","choices":[]}"#
        );

        // 5.0.0：`index` 为 `None` ⇒ 省略该键（此前写 `"index":0`，与真的 `index: 0` 撞形）。
        let tool_call = OpenAIStreamToolCall::default();
        assert_eq!(
            serde_json::to_string(&tool_call).unwrap_or_default(),
            r#"{"function":{"arguments":""}}"#
        );
        assert_eq!(
            serde_json::to_string(&OpenAIStreamToolCall {
                index: Some(0),
                ..Default::default()
            })
            .unwrap_or_default(),
            r#"{"index":0,"function":{"arguments":""}}"#
        );

        // 流式 usage：全 `Option` + 全省略，空对象也是合法形态。
        assert_eq!(
            serde_json::to_string(&OpenAIStreamUsage::default()).unwrap_or_default(),
            "{}"
        );
    }

    /// R-12：直接按公开类型解流式帧的库用户不该被非流式 [`OpenAIUsage`] 的必填计数打死。
    /// 线上 `{"prompt_tokens":1,"completion_tokens":2}`（无 `total_tokens`）是常见形态。
    ///
    /// 断言刻意**不解引用 `chunk.usage` 的字段**，而是把它序列化回 JSON 再比：把
    /// `OpenAIStreamChunk::usage` 的类型改回 `Option<OpenAIUsage>` 时，这条是**运行期**红
    /// （`missing field \`total_tokens\``）而不是编译不过 —— 编译不过会把下一条正向对照一起拖红，
    /// 「红了」与「看不清为什么红」就成了同一件事。同一条断言还顺带钉住「缺席的计数不被补成 0」。
    #[test]
    fn stream_usage_type_accepts_partial_counts() {
        let partial = r#"{"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2}}"#;
        let chunk: OpenAIStreamChunk =
            serde_json::from_str(partial).expect("partial usage counts must decode");
        let usage = serde_json::to_value(&chunk.usage).unwrap_or(Value::Null);
        assert_eq!(
            usage,
            serde_json::json!({ "prompt_tokens": 1, "completion_tokens": 2 })
        );
    }

    /// 上一条的正向对照，**独立用例**：三计数齐全（外加线上真有的两个明细子对象）同样解得动，
    /// 且计数原样读出 —— 证明上一条钉的是「计数可缺席」，不是「usage 被整个忽略了」。
    ///
    /// 直接解 [`OpenAIStreamUsage`] 而不经 [`OpenAIStreamChunk`]：上一条的篡改动的是 chunk 上那个
    /// 字段的类型，本条据此仍能编译、仍该绿。
    #[test]
    fn stream_usage_type_still_reads_full_counts_and_details() {
        let full = r#"{"prompt_tokens":13171,"completion_tokens":16,"total_tokens":13187,"completion_tokens_details":{"reasoning_tokens":14},"prompt_tokens_details":{"cached_tokens":13056}}"#;
        let usage: OpenAIStreamUsage = serde_json::from_str(full).expect("full usage must decode");
        assert_eq!(usage.prompt_tokens, Some(13171));
        assert_eq!(usage.completion_tokens, Some(16));
        assert_eq!(usage.total_tokens, Some(13187));
        assert_eq!(
            usage.prompt_tokens_details.and_then(|d| d.cached_tokens),
            Some(13056)
        );
        assert_eq!(
            usage
                .completion_tokens_details
                .and_then(|d| d.reasoning_tokens),
            Some(14)
        );
    }

    /// C-4：非流式响应缺 `choices` 不该让整个响应解码失败。实测此前三个入口同时报
    /// `missing field \`choices\``（`from_str` / `parse_response` / `parse_openai_response_to_anthropic`）。
    #[test]
    fn non_stream_response_without_choices_decodes_as_empty() {
        let no_choices = r#"{"id":"c1","object":"chat.completion","model":"m","usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#;
        let resp: OpenAIChatResponse =
            serde_json::from_str(no_choices).expect("missing choices must decode");
        assert!(resp.choices.is_empty());
        // 其余字段照常转换 —— 「不报错」不是靠把整个响应丢掉换来的。
        assert_eq!(resp.id, "c1");
        assert_eq!(resp.model, "m");
        assert_eq!(resp.usage.prompt_tokens, 3);
        assert_eq!(resp.usage.total_tokens, 8);
    }

    /// C-4 的另一半：`"choices": null` 与缺席是同一件事（上游说「我没有 choices」），不该报
    /// `invalid type: null, expected a sequence`。`#[serde(default)]` 只管缺席，显式 null 要靠
    /// [`de_choices_or_empty`]。三份对齐：TS 的 `Array.isArray` 守卫与 Go 的 nil slice 都容忍，
    /// 此前只有 Rust 是异类。
    #[test]
    fn non_stream_response_with_null_choices_decodes_as_empty() {
        let null_choices = r#"{"id":"c1","object":"chat.completion","model":"m","choices":null,"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#;
        let resp: OpenAIChatResponse =
            serde_json::from_str(null_choices).expect("null choices must decode");
        assert!(resp.choices.is_empty());
        assert_eq!(resp.id, "c1");
        assert_eq!(resp.usage.prompt_tokens, 3);
    }

    /// 宽容度的边界，**独立用例**：只收 `null`，对象形态的 `choices` 仍然报错。非流式响应体整个
    /// 不是 choices 数组 = 这次调用彻底坏掉，不该被悄悄抹成「没有内容」（理由见
    /// [`de_choices_or_empty`]；流式那侧刻意宽一档，见 [`de_choices_lenient`]）。
    /// 这条同时是「null 被收下」不是靠「什么都收」换来的证明。
    #[test]
    fn non_stream_response_with_object_choices_is_still_an_error() {
        let object_choices =
            r#"{"id":"c1","object":"chat.completion","model":"m","choices":{"0":{}}}"#;
        let err = serde_json::from_str::<OpenAIChatResponse>(object_choices)
            .expect_err("object choices must stay an error");
        assert!(err.to_string().contains("expected a sequence"), "{err}");
    }

    /// 上几条的正向对照，**独立用例**：`choices` 在场时元素被如实读出 —— 证明它们钉的是
    /// 「缺席 / null 按空数组」，不是「choices 根本没被解析」。
    #[test]
    fn non_stream_response_with_choices_still_reads_them() {
        let with_choices = r#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#;
        let resp: OpenAIChatResponse =
            serde_json::from_str(with_choices).expect("choices present must decode");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content, "hi");
        assert_eq!(resp.choices[0].finish_reason, "stop");
    }

    /// C-2：非流式响应缺 `usage` 不该让整个响应解码失败。不结算用量的兼容实现、以及被上游省略
    /// `usage` 的回包都是这个形态；正文明明在，此前调用方什么都拿不到。
    #[test]
    fn non_stream_response_without_usage_decodes_with_zero_counts() {
        let no_usage = r#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}]}"#;
        let resp: OpenAIChatResponse =
            serde_json::from_str(no_usage).expect("missing usage must decode");
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 0);
        assert_eq!(resp.usage.total_tokens, 0);
        // 正文没被这一步吃掉。
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content, "hi");
    }

    /// C-2 的另一半：`"usage": null` 与缺席是同一件事（上游说「我没有用量数据」），不该报
    /// `invalid type: null, expected struct OpenAIUsage`。`#[serde(default)]` 只管缺席，
    /// 显式 null 要靠 [`de_usage_or_default`]。
    #[test]
    fn non_stream_response_with_null_usage_decodes_with_zero_counts() {
        let null_usage = r#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":null}"#;
        let resp: OpenAIChatResponse =
            serde_json::from_str(null_usage).expect("null usage must decode");
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 0);
        assert_eq!(resp.usage.total_tokens, 0);
        // 正文没被这一步吃掉。
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content, "hi");
    }

    /// 上两条的正向对照，**独立用例**：usage 在场时三个计数被如实读出 —— 证明它们钉的是
    /// 「缺席 / null 按 0」，不是「usage 根本没被读」。
    #[test]
    fn non_stream_response_with_usage_reads_the_counts() {
        let with_usage = r#"{"id":"c1","object":"chat.completion","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":9,"total_tokens":16}}"#;
        let resp: OpenAIChatResponse =
            serde_json::from_str(with_usage).expect("usage present must decode");
        assert_eq!(resp.usage.prompt_tokens, 7);
        assert_eq!(resp.usage.completion_tokens, 9);
        assert_eq!(resp.usage.total_tokens, 16);
    }

    /// 流式那侧 [`de_choices_lenient`] 的宽容面**逐形态**钉住。此前只有「对象」形态被测到
    /// （`non_array_choices_yields_zero_events_not_a_stream_error`），`null` / 标量 / 缺席三种
    /// 从未被任何用例走过 —— 「它收 null」当时是读代码读出来的，不是跑出来的。
    ///
    /// 这条同时是非流式那侧刻意窄一档的对照：两个函数的宽容面不同是**有意**的，不是漂移。
    #[test]
    fn stream_choices_accepts_every_non_array_shape() {
        for body in [
            r#"{"id":"c1","choices":null}"#,
            r#"{"id":"c1"}"#,
            r#"{"id":"c1","choices":{"0":{}}}"#,
            r#"{"id":"c1","choices":123}"#,
        ] {
            let chunk: OpenAIStreamChunk =
                serde_json::from_str(body).unwrap_or_else(|e| panic!("{body} must decode: {e}"));
            assert!(chunk.choices.is_empty(), "{body}");
            // 「没报错」不是靠把整帧丢掉换来的：同帧的其它字段照常读出。
            assert_eq!(chunk.id, "c1", "{body}");
        }
    }

    /// 上一条的正向对照，**独立用例**：真数组照常解出元素 —— 证明上一条钉的是「非数组落空」，
    /// 不是「流式 choices 永远是空的」。
    #[test]
    fn stream_choices_still_decodes_a_real_array() {
        let real = r#"{"id":"c1","choices":[{"index":0,"delta":{"content":"hi"}}]}"#;
        let chunk: OpenAIStreamChunk = serde_json::from_str(real).expect("real array must decode");
        assert_eq!(chunk.choices.len(), 1);
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("hi"));
    }

    /// C-1：choice 缺 `delta` 不该终止整条流。此前必填，缺它的帧报 `missing field \`delta\``。
    #[test]
    fn stream_choice_without_delta_decodes_to_an_empty_delta() {
        let no_delta = r#"{"id":"c1","choices":[{"index":0,"finish_reason":"stop"}]}"#;
        let chunk: OpenAIStreamChunk =
            serde_json::from_str(no_delta).expect("missing delta must decode");
        let choice = chunk.choices.first().expect("choice present");
        assert!(choice.delta.role.is_none());
        assert!(choice.delta.content.is_none());
        assert!(choice.delta.reasoning_content.is_none());
        assert!(choice.delta.tool_calls.is_none());
        assert_eq!(choice.finish_reason.as_deref(), Some("stop"));
    }

    /// 上一条的正向对照，**独立用例**：delta 在场时内容被如实读出 —— 证明上一条钉的是
    /// 「delta 可缺席」，不是「delta 整个被丢掉了」。
    #[test]
    fn stream_choice_with_delta_still_reads_its_content() {
        let with_delta = r#"{"id":"c1","choices":[{"index":0,"delta":{"content":"hi"}}]}"#;
        let chunk: OpenAIStreamChunk =
            serde_json::from_str(with_delta).expect("delta present must decode");
        let choice = chunk.choices.first().expect("choice present");
        assert_eq!(choice.delta.content.as_deref(), Some("hi"));
    }

    /// R-3：tool_call 的 `index` 缺席与 `index: 0` 必须可分辨 —— 归并键的降级判据全靠这一点。
    #[test]
    fn stream_tool_call_index_absent_is_not_zero() {
        let absent: OpenAIStreamToolCall =
            serde_json::from_str(r#"{"id":"call_1","function":{"arguments":"{}"}}"#)
                .expect("absent index must decode");
        assert_eq!(absent.index, None);
        assert_eq!(absent.function.name, None);

        // 正向对照：真的 `index: 0` 读出 `Some(0)`，不与缺席同形。
        let zero: OpenAIStreamToolCall =
            serde_json::from_str(r#"{"index":0,"function":{"name":"f","arguments":"{}"}}"#)
                .expect("explicit index must decode");
        assert_eq!(zero.index, Some(0));
        assert_eq!(zero.function.name.as_deref(), Some("f"));
    }
}
