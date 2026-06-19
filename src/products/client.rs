//! 商品中心业务方法。端口自 `products/client.ts`（declaration-merging → `impl Client` 块）。
//!
//! 端点：tk-dist `/api/distribution/public/products/{by-family,by-slug}`，经 Go 反代。

use super::types::{Audience, Product, ProductFamily, RegionScope};
use crate::billing::entitlements::urlencoding;
use crate::core::client::Client;
use crate::shared::{Error, Result};
use tokio_util::sync::CancellationToken;

impl Client {
    /// 按商品族查询在售商品。全参可空（后端无过滤即返全部在售）。对应 TS `listProductsByFamily`。
    ///
    /// `region` 命中时同时返回 GLOBAL 商品。空 data → 空数组。
    pub async fn list_products_by_family(
        &self,
        family: Option<ProductFamily>,
        audience: Option<Audience>,
        region: Option<RegionScope>,
        signal: Option<CancellationToken>,
    ) -> Result<Vec<Product>> {
        let mut params: Vec<String> = Vec::new();
        if let Some(f) = family {
            params.push(format!("family={}", urlencoding(f.as_str())));
        }
        if let Some(a) = audience {
            params.push(format!("audience={}", urlencoding(a.as_str())));
        }
        if let Some(r) = region {
            params.push(format!("region={}", urlencoding(r.as_str())));
        }
        let path = if params.is_empty() {
            "/distribution/public/products/by-family".to_string()
        } else {
            format!(
                "/distribution/public/products/by-family?{}",
                params.join("&")
            )
        };
        self.commerce_get_list(&path, signal).await
    }

    /// 按 slug 查询单个在售商品。slug = biz_product_id（UNIQUE）。对应 TS `getProductBySlug`。
    ///
    /// 找不到或已下架 → 抛 [`Error::Http`] 404（或空 data → Err）。
    pub async fn get_product_by_slug(
        &self,
        slug: &str,
        region: Option<RegionScope>,
        signal: Option<CancellationToken>,
    ) -> Result<Product> {
        if slug.is_empty() {
            return Err(Error::other("getProductBySlug: slug is required"));
        }
        let path = match region {
            Some(r) => format!(
                "/distribution/public/products/by-slug/{}?region={}",
                urlencoding(slug),
                urlencoding(r.as_str())
            ),
            None => format!(
                "/distribution/public/products/by-slug/{}",
                urlencoding(slug)
            ),
        };
        // TS: `if (!resp.data) throw new Error(...)` —— 空 data 抛 Err。
        self.commerce_get_opt::<Product>(&path, signal)
            .await?
            .ok_or_else(|| {
                Error::other(format!("getProductBySlug: product not found (slug={slug})"))
            })
    }
}
