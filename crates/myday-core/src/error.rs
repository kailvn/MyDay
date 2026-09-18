//! 统一错误类型。
//!
//! [`ErrorCode`] 同时是 CLI 退出码与 IPC/JSON 错误码的来源：
//! 0 成功；1 一般错误；2 参数错误；3 未找到；4 冲突。
//! CLI 与 JSON 字段保持向后兼容（需求 §2.3、§七.9）。

use std::fmt;

/// 稳定错误码，与 CLI 退出码一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// 一般内部错误（存储、IO 等），退出码 1
    Internal,
    /// 参数 / 数据校验错误，退出码 2
    Invalid,
    /// 目标条目不存在，退出码 3
    NotFound,
    /// 冲突（如幂等键命中不同条目、时间冲突），退出码 4
    Conflict,
    /// 尚未实现（P1 及以后），退出码 1
    Unsupported,
}

impl ErrorCode {
    /// 对应的进程退出码。
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorCode::Internal | ErrorCode::Unsupported => 1,
            ErrorCode::Invalid => 2,
            ErrorCode::NotFound => 3,
            ErrorCode::Conflict => 4,
        }
    }

    /// 稳定的字符串形式，出现在 JSON 的 `error.code` 字段中。
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Internal => "INTERNAL",
            ErrorCode::Invalid => "INVALID",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::Conflict => "CONFLICT",
            ErrorCode::Unsupported => "UNSUPPORTED",
        }
    }
}

/// 核心库统一错误。
#[derive(Debug, thiserror::Error)]
pub enum MyDayError {
    #[error("{0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("{0}")]
    Internal(String),
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

impl MyDayError {
    pub fn code(&self) -> ErrorCode {
        match self {
            MyDayError::Invalid(_) => ErrorCode::Invalid,
            MyDayError::NotFound(_) => ErrorCode::NotFound,
            MyDayError::Conflict(_) => ErrorCode::Conflict,
            MyDayError::Unsupported(_) => ErrorCode::Unsupported,
            MyDayError::Internal(_) => ErrorCode::Internal,
            MyDayError::Storage(_) | MyDayError::Io(_) | MyDayError::Serde(_) | MyDayError::Zip(_) => {
                ErrorCode::Internal
            }
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub type Result<T> = std::result::Result<T, MyDayError>;
