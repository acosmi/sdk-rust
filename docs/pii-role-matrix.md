# PII 角色可见性矩阵（Rust）

> 适用：`acosmi-sdk`（Rust）全版本。
> 行为契约源自主仓 K10a PII Aspect（`@FieldEncrypt` / `@Sensitive` 切面）+ IMPL-A 角色严格化，
> 随 `@acosmi/sdk-ts` v1.9.0 发布；Rust SDK 端口自 `acosmi-sdk-ts/docs/pii-role-matrix.md`。

## 1. 背景

主仓（tk-dist Java）完成两条根因修复，决定了调用方拿到的字段是明文还是脱敏：

- **PII Aspect**：finance 表族（发票 / 对公转账 / 退款记录）+ 法律表族 + 企业席位表族的敏感列改走
  `@FieldEncrypt` 切面真加密落盘。

- **角色严格化**（`SensitiveSerializer.normalizeAuthority`）：通用 `ROLE_ADMIN` / `ROLE_USER` /
  `INTERNAL` 三个老别名的 fail-OPEN 隐患已根治。调用方传的 token 必须真有正式角色，否则视同 guest。

**SDK 边界**：Rust 类型只携带字段并（在 finance 域）以文档注释标注 PII 级别——**不内置脱敏逻辑**。
真实脱敏由后端 `PiiDesensitizer` 按角色返回不同字符串（明文 / `***1234` / 全 `***`）。
SDK 调用方（Web / Desktop / CLI）需理解角色矩阵以正确渲染。

## 2. 四角色

来源：主仓 `SensitiveSerializer.normalizeAuthority`。

| 角色常量 | 含义 | 典型颁发方 |
| --- | --- | --- |
| `ROLE_PLATFORM_ADMIN` | 平台管理员（跨租户）——后台运营 / 客服 / 财务工作台 | 主仓 admin 登录 + claim `platform_admin` |
| `ROLE_S2S` | 服务对服务——Go 网关 / 微服务内部互调 | S2S Token（`X-Service-Secret`） |
| `ROLE_LAWYER` | 律师——仅查看自己执业资料 + 自己接单的案件 | C 端登录 + 律师资质审核通过 |
| `ROLE_CONSUMER` | 消费者——普通 C 端用户，仅查看自己的数据 | C 端 OAuth（`/api/consumer/**`） |
| （guest / unknown） | 匿名 / 未登录 / 角色不匹配 | 无 token / Bearer 无效 / 角色字符串非上述 4 个 |

## 3. PII 级 × 角色矩阵

PII 分 3 级。**Rust SDK 用 `L0` / `L2` / `L3` 命名**（见 `src/finance/types.rs` 的字段注释），
与后端 `@Sensitive(level=...)` 对齐——其中 `L0` 对应「公开」（部分后端文档记作 `L1`）：

- **L0 公开**——任何场景明文（`id` / `order_id` / `amount_fen` / `status` / `create_time` 等）。
- **L2 半遮**——部分场景半遮（`title` / `contact_address` / `nickname` / `avatar_url` 等）。
- **L3 仅 admin**——强敏感字段，默认全脱敏（`tax_id` / `bank_account` / `contact_phone` /
  `bank_name` / `license_no` / `id_card_no` 等）。

| 角色 | L0 公开 | L2 半遮 | L3 仅 admin |
| --- | --- | --- | --- |
| `ROLE_PLATFORM_ADMIN` | 明文 | 明文 | 明文 |
| `ROLE_S2S` | 明文 | 明文 | 明文 |
| `ROLE_LAWYER` | 明文 | 明文 | 脱敏（`***1234`） |
| `ROLE_CONSUMER` | 明文 | 明文 | 脱敏（`***1234`） |
| （guest / unknown） | 明文 | 脱敏 | 脱敏 |

> **Note**：`ROLE_LAWYER` / `ROLE_CONSUMER` 自己的数据，其 L3 字段仍可通过专门的 admin 直读端点
> （`/api/distribution/**/me/decrypted`）解密；但默认 `list_my_*` 端点走脱敏视图。SDK 类型上不强制——
> 调用方按上下文判定。

## 4. Breaking Change——旧别名失效

主仓 IMPL-A 角色严格化把以下 3 条 fail-OPEN 别名根治：

| 旧别名 | 误升级到（旧行为） | 现处理 |
| --- | --- | --- |
| `ROLE_ADMIN` | `platform_admin`（L3 明文） | 视同 `guest`（L2/L3 全脱敏） |
| `ROLE_USER` | `consumer`（L0/L2 明文） | 视同 `guest`（L2/L3 全脱敏） |
| `INTERNAL` | `s2s`（L3 明文） | 视同 `guest`（L2/L3 全脱敏） |

### 集成方应对

1. **token 升级到正式角色**：admin 后台 token 改派发 `ROLE_PLATFORM_ADMIN`；微服务 S2S 调用切到
   `ROLE_S2S` claim；C 端 OAuth 已自动派发 `ROLE_CONSUMER`。
2. **fallback 走 guest 视角**：灰度期旧 token 调 `/api/distribution/admin/**` 会拿到脱敏数据——
   这是设计内行为，不是 bug。
3. **角色字符串大小写敏感**：必须全大写 `ROLE_PLATFORM_ADMIN`，小写 / 缺前缀的 `platform_admin`
   会被 `normalizeAuthority` 拒绝。

## 5. Rust 类型实际字段

下表用 **Rust 端真实字段名**（snake_case）。注意 Rust 公开视图类型与 TS 文档的示意清单有差异——
**强敏感 L3 字段在公开 summary 视图上可能根本不携带（已剥离）**，只在 admin 直读端点出现。

### finance（`src/finance/types.rs` — `Invoice`）

`Invoice` 是 Rust SDK 中**唯一**带逐字段 PII 级注释的类型（其余域类型只在类型级 doc-comment 描述）：

| 字段（Rust） | 级别 | admin 明文示例 | guest 视图 |
| --- | --- | --- | --- |
| `id` / `invoice_no` / `order_id` / `amount_fen` / `status` | L0 | `INV20260525001` | 同（明文） |
| `title` | L2 | `Acosmi Tech Ltd` | `Acos*****td` |
| `contact_address` | L2 | `北京市朝阳区…` | `北京市*****` |
| `tax_id` | L3 | `91110108MA01ABC123` | `91**********23` |
| `bank_account` | L3 | `6225882104567890` | `6225********7890` |
| `bank_name` | L3 | `招商银行北京分行` | `招商***` |
| `contact_phone` | L3 | `13800001234` | `138****1234` |

> `amount_fen` 是整数分（`i64`，金额阵营 §3）；`tax_rate` 是 `f64` 比率，**不**是金额。两者都不脱敏。

### enterprise（`src/enterprise/types.rs` — `EnterpriseSummary`）

类型 doc-comment：「PII L3 字段 `contact_phone` / `contact_email` 仅 OWNER/ADMIN 可见」。
公开视图实际携带的字段：

| 字段（Rust） | 级别 |
| --- | --- |
| `id` / `org_name` / `status` | L0 |
| `org_code`（统一社会信用代码） / `legal_representative` | L2 |
| `contact_phone` / `contact_email` | L3（仅 OWNER/ADMIN 可见） |

> Rust `EnterpriseSummary` 用 `org_code`（不是 TS 文档示意的 `creditCode`）。非 OWNER/ADMIN 角色下
> 后端对 L3 字段返回脱敏值。

### casehall（`src/casehall/types.rs` — `LawyerSummary` / `CaseLead`）

类型 doc-comment：「PII L3 字段 `license_no` 等已脱敏剥离」。这是关键差异——

| 字段（Rust） | 级别 | 说明 |
| --- | --- | --- |
| `LawyerSummary.real_name` | L2 | 公开 `list_lawyers` 半遮（`张*丰`） |
| `LawyerSummary`：`license_no` / `id_card_no` | L3 | **公开视图不携带该字段**（已剥离），仅 admin 直读端点出现 |
| `CaseLead.id` / `title` / `status` / `budget_fen` | L0 | 明文 |
| `CaseLead`：`contact_phone` | L3 | **公开 `CaseLead` 不携带该字段**（已剥离） |

> 与 TS 示意清单不同：Rust `LawyerSummary` 结构体里**没有** `license_no` / `id_card_no` 字段，
> `CaseLead` 里**没有** `contact_phone` 字段。这些 L3 字段被后端在序列化公开视图前剥离，因此 SDK 反序列化
> 时它们根本不存在——比「字段在但被脱敏」更强的隐私保证。需要解密值时走 admin 直读端点（独立 DTO）。

## 6. 脱敏算法引用

后端 `PiiDesensitizer` 按字段类型自动派生脱敏策略：

| 策略 | 输入 | 输出 |
| --- | --- | --- |
| `PHONE` | `13800001234` | `138****1234` |
| `EMAIL` | `user@acosmi.com` | `u***@acosmi.com` |
| `ID_CARD` | `110101199001011234` | `1101**********1234` |
| `BANK_CARD` | `6225882104567890` | `6225********7890` |
| `NAME` | `张三丰` | `张*丰` |
| `ADDRESS` | `北京市朝阳区…` | `北京市*****` |
| `GENERIC` | 任意其他 L2/L3 字符串 | `XX*****XX`（前后 2 字符 + 中间星号） |

`@Sensitive(strategy=...)` 显式指定时优先；否则按字段名启发（含 `phone`/`mobile` → `PHONE`，
含 `email` → `EMAIL`，等）。

## 7. 集成方应做的 mock

Rust 集成方（Web / Desktop / 微服务）在集成测试中应 mock 不同角色 token，验证渲染层正确处理角色差异。
Rust SDK 通过 `Config.store`（`Arc<dyn TokenStore>`）注入预置 token，而非像 TS 那样直接传 `token` 字段：

```rust,no_run
use std::sync::Arc;
use acosmi::{Client, Config};
use acosmi::core::InMemoryTokenStore;

// 预置一个携带 ROLE_CONSUMER claim 的 access token（测试夹具按需自铸）。
# fn mint_token_with_role(_role: &str) -> String { String::new() }
# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let store = Arc::new(InMemoryTokenStore::new());
// store.save(...) 写入预置 token —— 具体方法见 TokenStore trait（src/core/store.rs）。

let client = Client::create(Config {
    server_url: Some(std::env::var("ACOSMI_SERVER_URL")?),
    store: Some(store),
    ..Default::default()
})
.await?;

// consumer 角色：L3 字段脱敏（6225********7890）。list_my_invoices 只接 signal 一个参。
let invoices = client.list_my_invoices(None).await?; // Vec<Invoice>
if let Some(inv) = invoices.first() {
    // assert: inv.bank_account 形如 ^\d{4}\*+\d{4}$
    let _ = &inv.bank_account;
}

// guest（无 token）调公开列表：L2 半遮、L3 字段已被后端剥离。
// list_lawyers(params, signal) 返回 Vec<LawyerSummary>（非分页）。
let lawyers = client.list_lawyers(None, None).await?;
if let Some(lw) = lawyers.first() {
    // assert: lw.real_name 含 '*'（L2 半遮）；LawyerSummary 上无 license_no 字段（L3 剥离）。
    let _ = &lw.real_name;
}
# Ok(()) }
```

验证要点：

- `ROLE_PLATFORM_ADMIN` / `ROLE_S2S`：L3 字段明文。
- `ROLE_CONSUMER` / `ROLE_LAWYER`：自己的数据 L0/L2 明文，L3 脱敏（`***1234`）。
- guest（无 token）：L2 半遮、L3 脱敏；且公开 summary 类型上的 L3 字段在 Rust 端**不存在**（编译期即无）。
- `ROLE_ADMIN`（旧别名）：视同 guest——L3 脱敏。集成测试应覆盖这条防回归。

## 8. 参考

- PII 行为契约源自主仓（tk-dist Java）的 PII 加密切面、角色规范化器（`SensitiveSerializer`）、
  脱敏器（`PiiDesensitizer`）；以及 finance 表族加密迁移。这些是后端实现，SDK 调用方不可见也无需关心
  ——只需理解本文档的角色矩阵。
- Rust SDK 类型注释：`src/finance/types.rs` 的 `Invoice` doc-comment（逐字段 L0/L2/L3）。
- 相关 Rust 读方法：`Client::list_my_invoices` / `Client::list_lawyers` / `Client::get_enterprise` /
  `Client::list_my_case_leads`。
