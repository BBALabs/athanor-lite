use thiserror::Error;

/// Unified error type crossing the IPC boundary.
///
/// Serialized as `{ code, message }` so the frontend can map every failure to a
/// designed error state instead of showing raw strings.
#[derive(Debug, Error)]
pub enum AthanorError {
    #[error("hardware probe failed: {0}")]
    Hardware(String),

    #[error("workspace error: {0}")]
    Workspace(String),

    #[error("model catalog error: {0}")]
    Catalog(String),

    #[error("download error: {0}")]
    Download(String),

    #[error("runtime error: {0}")]
    Runtime(String),

    #[error("chat error: {0}")]
    Chat(String),

    #[error("knowledge base error: {0}")]
    Rag(String),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("app path resolution failed: {0}")]
    Path(String),
}

impl AthanorError {
    pub fn code(&self) -> &'static str {
        match self {
            AthanorError::Hardware(_) => "HARDWARE",
            AthanorError::Workspace(_) => "WORKSPACE",
            AthanorError::Catalog(_) => "CATALOG",
            AthanorError::Download(_) => "DOWNLOAD",
            AthanorError::Runtime(_) => "RUNTIME",
            AthanorError::Chat(_) => "CHAT",
            AthanorError::Rag(_) => "RAG",
            AthanorError::Mcp(_) => "MCP",
            AthanorError::Io(_) => "IO",
            AthanorError::Serde(_) => "SERDE",
            AthanorError::Path(_) => "PATH",
        }
    }
}

impl serde::Serialize for AthanorError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("AthanorError", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, AthanorError>;
