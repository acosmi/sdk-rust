//! 跨域基础设施：错误体系、APIResponse / 分页结构、跨域共享 DTO
//! （分页 / operation / retryAdvice / principal / gate）。
//!
//! 对齐 `shared/index.ts` 的逐文件 re-export。

pub mod api_response;
pub mod errors;
pub mod gate;
pub mod operation;
pub mod pagination;
pub mod principal;
pub mod retry_advice;

pub use api_response::{ApiResponse, YudaoPageResult};
pub use errors::{Error, HttpError, NetworkError, Result, StreamError};
pub use gate::{
    BillingPreflightResult, FeatureGateState, FeatureGateStatus, GateQuota, StepUpStatus,
};
pub use operation::{
    IdempotencyKey, OperationId, OperationSource, OperationStatus, ProviderRequestStatus,
    VerifyStatus, IDEMPOTENCY_KEY_HEADER,
};
pub use pagination::{PageRequest, PageResult, SortDirection};
pub use principal::{ApiClientRef, PrincipalRef, TenantRef};
pub use retry_advice::{
    retry_reason_for_oauth_error, RetryAdvice, RetryAdviceReason, RETRY_ADVICE_REASONS,
};
