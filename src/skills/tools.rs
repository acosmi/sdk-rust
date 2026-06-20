//! 统一工具（Unified Tools）业务方法。端口自 `skills/tools.ts`
//! （declaration-merging → 此处 `impl Client` 块）。

use super::types::{ToolListResponse, ToolView};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::shared::Result;
use tokio_util::sync::CancellationToken;

impl Client {
    /// 获取当前用户租户下的所有工具（Skill 优先 + Plugin 兜底）。对应 TS `listTools`。
    pub async fn list_tools(&self, signal: Option<CancellationToken>) -> Result<Vec<ToolView>> {
        let resp: ToolListResponse = self.billing_get("/tools", signal).await?;
        Ok(resp.skills)
    }

    /// 获取单个工具详情。对应 TS `getTool`。
    pub async fn get_tool(
        &self,
        tool_id: &str,
        signal: Option<CancellationToken>,
    ) -> Result<ToolView> {
        self.billing_get(&format!("/tools/{}", urlencoding(tool_id)), signal)
            .await
    }
}
