# acosmi-sdk (Rust)

> Acosmi 模型网关 + Agent Run Gateway + Compliance 的 Rust SDK — 双格式（Anthropic + OpenAI），原生异步（`tokio` + `reqwest`/rustls）。

[![crates.io](https://img.shields.io/crates/v/acosmi-sdk.svg)](https://crates.io/crates/acosmi-sdk)

## 状态

- 端口自 [`@acosmi/sdk-ts`](https://github.com/acosmi/sdk-ts)（事实标准主实现）。当前对齐 **v2.8.0**。
- 仅原生运行时（`tokio` + `reqwest`，rustls TLS）；不提供 WASM/浏览器并列构建。
- 分阶段端口推进中（P0 脚手架 → 地基 → 模型/双 adapter → 各业务域）。

## 安装

```toml
[dependencies]
acosmi-sdk = "2.8"
tokio = { version = "1", features = ["full"] }
```

库名为 `acosmi`：

```rust
use acosmi::VERSION;
```

## 设计

- **双格式红线**：`AnthropicAdapter` + `OpenAIAdapter` 等地位，按托管模型的 `preferred_format` / `supported_formats` 选路，不可降级。
- **wire 契约**：字段名为 snake_case，与上游 JSON 0 偏差；类型/错误名跨语言一致。
- **bug-for-bug**：POST 默认不重试（计费安全）；流式路径不走重试（防双扣）；`thinking`/`redacted_thinking` 块在清洗时硬豁免；401 单次重试防递归。
- **金额三阵营**：钱包域 `f64`；finance/商品化 `*Fen` 为 `i64`（整数分）；`json.Number` 类十进制金额为 `String`（上层用 `rust_decimal` 解析）。

## Features

| feature | 默认 | 说明 |
|---------|------|------|
| `sanitize` | ✅ | 历史消息清洗子包（对齐 npm `./sanitize`） |
| `desktop-loopback` | — | 桌面 OAuth 的 loopback HTTP server |

## License

MIT © Acosmi
