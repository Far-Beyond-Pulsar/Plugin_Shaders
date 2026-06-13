//! Feature modules - each feature (nodes, connections, etc.) is self-contained
//!
//! Each feature module typically contains:
//! - types.rs: Feature-specific types
//! - operations.rs: Business logic and state mutations
//! - rendering.rs: GPUI rendering code
//! - panel.rs: Dockable panel (if applicable)

pub mod clipboard;
pub mod comments;
pub mod compilation;
pub mod connections;
pub mod nodes;
pub mod preview;
pub mod shader_model;
pub mod undo;
pub mod viewport;

/// Initialize all feature modules that need startup registration
pub fn initialize_features() {
    shader_model::standard::register_standard_models();
}
