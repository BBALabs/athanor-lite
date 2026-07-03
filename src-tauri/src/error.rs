use thiserror::Error;

/// Unified error type crossing the IPC boundary.
///
/// Serialized as `{ code, message }` so the frontend can map every failure to a
/// designed error state instead of showing raw strings.
#[derive(Debug, Error)]
pub enum CondereError {
    #[error("hardware probe failed: {0}")]
    Hardware(String),

    #[error("workspace error: {0}")]
    Workspace(String),

    #[error("model catalog error: {0}")]
    Catalog(String),

    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("app path resolution failed: {0}")]
    Path(String),
}

impl CondereError {
    pub fn code(&self) -> &'static str {
        match self {
            CondereError::Hardware(_) => "HARDWARE",
            CondereError::Workspace(_) => "WORKSPACE",
            CondereError::Catalog(_) => "CATALOG",
            CondereError::Io(_) => "IO",
            CondereError::Serde(_) => "SERDE",
            CondereError::Path(_) => "PATH",
        }
    }
}

impl serde::Serialize for CondereError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("CondereError", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, CondereError>;
