//! Serializable command error: the frontend always receives `{ code, message }`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<mimic_core::db::DbError> for CommandError {
    fn from(e: mimic_core::db::DbError) -> Self {
        let code = match &e {
            mimic_core::db::DbError::NotFound(_) => "not_found",
            mimic_core::db::DbError::Invalid(_) => "invalid",
            mimic_core::db::DbError::Busy(_) => "busy",
            mimic_core::db::DbError::Unconfirmed(_) => "confirm",
            _ => "database",
        };
        Self::new(code, e.to_string())
    }
}

impl From<mimic_core::jobs::JobError> for CommandError {
    fn from(e: mimic_core::jobs::JobError) -> Self {
        Self::new("job", e.to_string())
    }
}

impl From<mimic_core::sources::SourceError> for CommandError {
    fn from(e: mimic_core::sources::SourceError) -> Self {
        let code = match &e {
            mimic_core::sources::SourceError::Io(_) => "source_io",
            mimic_core::sources::SourceError::Malformed(_) => "source_malformed",
            mimic_core::sources::SourceError::UnknownConnector(_) => "unknown_connector",
            mimic_core::sources::SourceError::Aborted(_) => "canceled",
        };
        Self::new(code, e.to_string())
    }
}

impl From<mimic_core::providers::ProviderError> for CommandError {
    fn from(e: mimic_core::providers::ProviderError) -> Self {
        let code = match &e {
            mimic_core::providers::ProviderError::Config(_) => "provider_config",
            mimic_core::providers::ProviderError::Unreachable(_) => "provider_unreachable",
            mimic_core::providers::ProviderError::Refused(_) => "provider_refused",
            mimic_core::providers::ProviderError::Malformed(_) => "provider_malformed",
            mimic_core::providers::ProviderError::Unknown(_) => "provider_unknown",
        };
        Self::new(code, e.to_string())
    }
}

impl From<mimic_core::generation::GenerationError> for CommandError {
    fn from(e: mimic_core::generation::GenerationError) -> Self {
        match e {
            mimic_core::generation::GenerationError::Db(d) => d.into(),
            mimic_core::generation::GenerationError::Provider(p) => p.into(),
        }
    }
}

impl From<mimic_core::engine::EngineError> for CommandError {
    fn from(e: mimic_core::engine::EngineError) -> Self {
        let code = match &e {
            mimic_core::engine::EngineError::NotRunning(_) => "engine_not_running",
            mimic_core::engine::EngineError::Timeout(_) => "engine_timeout",
            mimic_core::engine::EngineError::Remote { code, .. } => {
                return Self::new(&format!("engine_{code}"), e.to_string())
            }
            _ => "engine",
        };
        Self::new(code, e.to_string())
    }
}

impl From<std::io::Error> for CommandError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}

impl From<serde_json::Error> for CommandError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("json", e.to_string())
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
