//! File I/O and persistence layer
//!
//! Handles saving and loading shader graph files, including:
//! - Shader graph serialization/deserialization
//! - Legacy format support
//! - Format conversion utilities
//! - Autosave functionality

pub mod formats;
pub mod legacy;
pub mod save_load;

// Re-export main types and functions
pub use formats::{
    deserialize_shader, serialize_shader_with_header, ShaderAsset, ShaderEditorState,
};
pub use legacy::{is_legacy_format, try_parse_legacy_format};
