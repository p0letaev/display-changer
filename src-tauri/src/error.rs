use serde::ser::{Serialize, SerializeStruct, Serializer};

/// Errors returned to the frontend as `{ kind, detail }`, same shape as
/// `display_core::Error`.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Display(#[from] display_core::Error),
    #[error("profile storage: {0}")]
    Storage(#[from] std::io::Error),
    #[error("hotkey: {0}")]
    Hotkey(String),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type CommandResult<T> = Result<T, CommandError>;

impl Serialize for CommandError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (kind, detail) = match self {
            CommandError::Display(e) => return e.serialize(serializer),
            CommandError::Storage(e) => ("storage", e.to_string()),
            CommandError::Hotkey(e) => ("hotkey", e.clone()),
            CommandError::NotFound(e) => ("notFound", e.clone()),
        };
        let mut s = serializer.serialize_struct("CommandError", 2)?;
        s.serialize_field("kind", kind)?;
        s.serialize_field("detail", &detail)?;
        s.end()
    }
}
