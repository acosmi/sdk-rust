//! Token 持久化。端口自 `core/store.ts`（其端口自 `acosmi-sdk-go/store.go`）。
//!
//! TS 跨端有 File / LocalStorage / InMemory 三实现；纯原生 Rust 省略 LocalStorage
//! （浏览器专属，wasm target 再加，见方案 §4.3 平台取舍表），保留 File / InMemory。
//!
//! ## TokenStore 对象安全（审计 F5 / 方案 §4）
//! TS 接口含可选泛型方法 `withLock?<T>(fn)`（跨进程临界区，refresh 轮换用）。
//! 泛型方法破坏 Rust trait 对象安全（`dyn TokenStore` 不可用），故改为对象安全的
//! RAII guard：[`TokenStore::lock`] 返回 `Box<dyn Send>`，Client 以
//! `let _g = store.lock().await?;` 包裹 `load → refresh → save`，guard drop 时释放。
//! 默认实现无锁（返回空 guard），对应 TS `withLock` 可选语义。

use crate::auth::types::TokenSet;
use crate::shared::errors::{Error, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// 跨进程文件锁配置（对齐 TS `fileLockDefaults`）。
#[derive(Debug, Clone, Copy)]
pub struct FileLockDefaults {
    /// 获取锁的超时上限（毫秒）。
    pub acquire_timeout_ms: u64,
    /// 旧锁判定阈值（毫秒）。锁文件 mtime 早于此值视为崩溃残留，自动 break。
    pub stale_ms: u64,
    /// 重试间隔基数（毫秒）。
    pub retry_base_ms: u64,
    /// 重试间隔抖动上限（毫秒）。
    pub retry_jitter_ms: u64,
}

/// 文件锁默认值。
pub const FILE_LOCK_DEFAULTS: FileLockDefaults = FileLockDefaults {
    acquire_timeout_ms: 30_000,
    stale_ms: 60_000,
    retry_base_ms: 30,
    retry_jitter_ms: 70,
};

/// Token 持久化接口。桌面智能体可自行实现（如 macOS Keychain）。
///
/// 跨端约定：
///   - `save`：写入持久化，失败返 `Err`；
///   - `load`：读取，不存在返 `Ok(None)`（与 Go IsNotExist 一致）；损坏/旧版本残留同样返 `Ok(None)`；
///   - `clear`：删除，不存在不报错（Logout 后 Clear 幂等）；
///   - `lock`：可选跨进程临界区（默认无锁）。
#[async_trait]
pub trait TokenStore: Send + Sync {
    async fn save(&self, tokens: &TokenSet) -> Result<()>;
    async fn load(&self) -> Result<Option<TokenSet>>;
    async fn clear(&self) -> Result<()>;

    /// 跨进程临界区锁。默认无锁（返回空 guard）。
    /// Client 在 refresh-rotation 时用 `let _g = store.lock().await?;` 包裹 load→refresh→save。
    async fn lock(&self) -> Result<Box<dyn Send>> {
        Ok(Box::new(()))
    }
}

// ============================================================================
// FileTokenStore —— 文件实现（~/.acosmi/tokens.json）
// ============================================================================

/// 基于文件的 token 存储（开发/测试用）。生产建议替换为系统钥匙串实现。
pub struct FileTokenStore {
    /// 自定义路径；`None` 则用默认 `~/.acosmi/tokens.json`。
    path: Option<PathBuf>,
    /// 进程内串行化（对齐 TS withChain；与跨进程 flock 配合）。
    chain: tokio::sync::Mutex<()>,
}

impl FileTokenStore {
    /// 创建文件 token 存储。`path` 为空则用默认路径。
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            chain: tokio::sync::Mutex::new(()),
        }
    }

    fn resolve_path(&self) -> Result<PathBuf> {
        if let Some(p) = &self.path {
            if !p.as_os_str().is_empty() {
                return Ok(p.clone());
            }
        }
        let home = home_dir()
            .ok_or_else(|| Error::other("FileTokenStore: cannot resolve home directory"))?;
        Ok(home.join(".acosmi").join("tokens.json"))
    }
}

/// 跨进程锁 guard：drop 时 best-effort 删除 sidecar `.lock` 文件。
struct FileLockGuard {
    lock_path: PathBuf,
}

impl Drop for FileLockGuard {
    fn drop(&mut self) {
        // best-effort —— 释放期间被 stale-break 删除是允许的。
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

#[async_trait]
impl TokenStore for FileTokenStore {
    async fn save(&self, tokens: &TokenSet) -> Result<()> {
        let _chain = self.chain.lock().await;
        let p = self.resolve_path()?;
        let dir = p
            .parent()
            .ok_or_else(|| Error::other("token path has no parent dir"))?
            .to_path_buf();
        let data = serde_json::to_vec_pretty(tokens)?;
        let p2 = p.clone();
        // 阻塞 fs（atomic rename + fsync）放进 blocking 线程池。
        tokio::task::spawn_blocking(move || -> Result<()> {
            use std::io::Write;
            std::fs::create_dir_all(&dir)?;
            let tmp = dir.join(format!(
                "tokens.json.tmp.{}.{}",
                std::process::id(),
                now_nanos()
            ));
            // 1) 写 tmp + fsync 文件内容。
            {
                let mut fh = std::fs::File::create(&tmp)?;
                fh.write_all(&data)?;
                fh.sync_all()?; // fsync
            }
            // 2) atomic rename。失败清理 tmp。
            if let Err(e) = std::fs::rename(&tmp, &p2) {
                let _ = std::fs::remove_file(&tmp);
                return Err(e.into());
            }
            // 3) 对目录 fsync（best-effort；部分平台不支持，吞掉）。
            if let Ok(dh) = std::fs::File::open(&dir) {
                let _ = dh.sync_all();
            }
            Ok(())
        })
        .await
        .map_err(|e| Error::other(format!("join save task: {e}")))?
    }

    async fn load(&self) -> Result<Option<TokenSet>> {
        let _chain = self.chain.lock().await;
        let p = self.resolve_path()?;
        let data = match tokio::fs::read(&p).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::other(format!("read token file: {e}"))),
        };
        // 损坏 / 截断 / 缺字段 / 旧版本残留 → 当作无 token（返 None），不报错。
        // serde 反序列化到非可选 String 字段天然等价 TS isValidTokenSet。
        Ok(serde_json::from_slice::<TokenSet>(&data).ok())
    }

    async fn clear(&self) -> Result<()> {
        let _chain = self.chain.lock().await;
        let p = self.resolve_path()?;
        match tokio::fs::remove_file(&p).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn lock(&self) -> Result<Box<dyn Send>> {
        let p = self.resolve_path()?;
        let lock_path = PathBuf::from(format!("{}.lock", p.display()));
        let dir = p
            .parent()
            .ok_or_else(|| Error::other("token path has no parent dir"))?
            .to_path_buf();
        tokio::fs::create_dir_all(&dir).await?;

        let start = SystemTime::now();
        loop {
            // 'create_new' = O_CREAT | O_EXCL：已存在返 AlreadyExists。
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut fh) => {
                    use std::io::Write;
                    let _ = write!(fh, "{}\n{}\n", std::process::id(), now_millis());
                    return Ok(Box::new(FileLockGuard { lock_path }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // stale 检测。
                    if let Ok(meta) = std::fs::metadata(&lock_path) {
                        if let Ok(modified) = meta.modified() {
                            if let Ok(age) = SystemTime::now().duration_since(modified) {
                                if age.as_millis() as u64 > FILE_LOCK_DEFAULTS.stale_ms {
                                    let _ = std::fs::remove_file(&lock_path);
                                    continue;
                                }
                            }
                        }
                    } else {
                        // 锁文件刚消失 → 立即重试。
                        continue;
                    }
                    if elapsed_ms(start) > FILE_LOCK_DEFAULTS.acquire_timeout_ms {
                        return Err(Error::other(format!(
                            "acquire token file lock timeout ({}ms): {}",
                            FILE_LOCK_DEFAULTS.acquire_timeout_ms,
                            lock_path.display()
                        )));
                    }
                    let jitter = now_nanos() % FILE_LOCK_DEFAULTS.retry_jitter_ms.max(1);
                    let wait = FILE_LOCK_DEFAULTS.retry_base_ms + jitter;
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// 创建文件 token 存储（与 Go `NewFileTokenStore` 等价）。
pub fn new_file_token_store(path: Option<PathBuf>) -> FileTokenStore {
    FileTokenStore::new(path)
}

// ============================================================================
// InMemoryTokenStore —— 兜底实现（不持久化）
// ============================================================================

/// 内存 token 存储（进程重启即丢失）。适合测试 / 短期调用 / 不落盘场景。
#[derive(Default)]
pub struct InMemoryTokenStore {
    tokens: std::sync::Mutex<Option<TokenSet>>,
}

impl InMemoryTokenStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl TokenStore for InMemoryTokenStore {
    async fn save(&self, tokens: &TokenSet) -> Result<()> {
        *self.tokens.lock().unwrap() = Some(tokens.clone());
        Ok(())
    }
    async fn load(&self) -> Result<Option<TokenSet>> {
        Ok(self.tokens.lock().unwrap().clone())
    }
    async fn clear(&self) -> Result<()> {
        *self.tokens.lock().unwrap() = None;
        Ok(())
    }
}

// ============================================================================
// 内部辅助
// ============================================================================

fn home_dir() -> Option<PathBuf> {
    // 避免新增 dep：unix 用 HOME，windows 用 USERPROFILE。
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn elapsed_ms(start: SystemTime) -> u64 {
    SystemTime::now()
        .duration_since(start)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
