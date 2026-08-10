use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Permission(String),
    #[error("{0}")]
    State(String),
    #[error("{0}")]
    Network(String),
    #[error("{0}")]
    Verification(String),
    #[error("{0}")]
    Service(String),
    #[error("{message}")]
    ServiceDiagnostic {
        message: String,
        raw_diagnostic: String,
    },
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Permission(_) => 3,
            Self::State(_) => 4,
            Self::Network(_) => 5,
            Self::Verification(_) => 6,
            Self::Service(_) | Self::ServiceDiagnostic { .. } => 7,
            Self::Internal(_) => 1,
        }
    }

    pub fn raw_diagnostic(&self) -> Option<&str> {
        match self {
            Self::ServiceDiagnostic { raw_diagnostic, .. } => Some(raw_diagnostic),
            _ => None,
        }
    }
}
