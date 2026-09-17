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

impl From<mimic_core::bridge::BridgeError> for CommandError {
    fn from(e: mimic_core::bridge::BridgeError) -> Self {
        let code = match &e {
            mimic_core::bridge::BridgeError::NotConnected => "lightroom_not_connected",
            mimic_core::bridge::BridgeError::Timeout(_) => "lightroom_timeout",
            mimic_core::bridge::BridgeError::Plugin { code, .. } => {
                return Self::new(&format!("lightroom_{code}"), e.to_string())
            }
            _ => "lightroom",
        };
        Self::new(code, e.to_string())
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
