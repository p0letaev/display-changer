//! Platform-independent display model, layout geometry and backends.
//!
//! Backends only read and apply state. Everything that can be shared between
//! platforms (validation, normalization, profile matching) lives here.

pub mod backend;
pub mod backends;
pub mod edid;
pub mod error;
pub mod layout;
pub mod model;
pub mod profile;

pub use backend::{Capabilities, DisplayBackend, OriginRule};
pub use error::{Error, LayoutIssue, Result};
pub use model::*;
pub use profile::{Profile, ProfileWarning, Resolved};
