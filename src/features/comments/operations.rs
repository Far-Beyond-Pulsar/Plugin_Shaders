//! Comment operations - drag, resize, edit, add, delete
//!
//! All comment operation methods have been migrated to the main ShaderEditorPanel
//! impl block in `src/editor/panel.rs`.
//!
//! ## Available Methods (implemented on ShaderEditorPanel)
//!
//! - `update_comment_drag()` - Update comment drag position
//! - `end_comment_drag()` - End comment drag and update contained nodes
//! - `update_comment_resize()` - Update comment resize based on handle being dragged
//! - `end_comment_resize()` - End comment resize and update contained nodes
//! - `finish_comment_editing()` - Finish editing comment text and save changes
//! - `create_comment_at_center()` - Create a new comment at the center of the current view
//! - `add_comment()` - Add a new comment at the specified position
//!
//! These methods are implemented as regular instance methods on ShaderEditorPanel,
//! not as trait methods, for better integration with the rest of the editor.

// Re-export ResizeHandle from panel module for convenience
pub use crate::editor::panel::ResizeHandle;
