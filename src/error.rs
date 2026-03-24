use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("CLI error: {0}")]
    Cli(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::Cli(_) => 2,
            AppError::Network(_) => 3,
            AppError::Protocol(_) => 4,
            AppError::Execution(_) => 1,
            AppError::Io(_) => 5,
            AppError::Json(_) => 6,
            AppError::Other(_) => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
