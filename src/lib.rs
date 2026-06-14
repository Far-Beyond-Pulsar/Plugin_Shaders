#![recursion_limit = "512"]

//! # Shader Graph Editor Plugin
//!
//! Visual shader graph editor for creating materials through node-based programming.
//! Adapts the blueprint editor architecture for WGSL shader authoring.
//!
//! ## Architecture
//!
//! - **core**: Core data types (BlueprintNode, BlueprintGraph, Connection, etc.)
//! - **editor**: Main editor state container and lifecycle management
//! - **features**: Feature modules (nodes, connections, comments, compilation, preview)
//! - **rendering**: Visual rendering layer (graph canvas, input handling, styling)
//! - **ui**: Reusable UI components and panels
//! - **io**: File I/O and persistence

use gpui::*;
use plugin_editor_api::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;
use std::{path::PathBuf, sync::Arc};
use ui::dock::PanelView;

// Module declarations
mod ai_tools;
mod core;
mod editor;
mod features;
mod io;
mod rendering;
mod ui_components;

// Re-export main types for plugin API compatibility
pub use core::definitions::*;
pub use core::events::*;
pub use core::graph::*;
pub use core::types::*;
pub use editor::panel::ShaderEditorPanel;

pub fn upsert_ai_session(file_path: PathBuf, graph: BlueprintGraph) {
    ai_tools::upsert_session(file_path, graph);
}

pub fn execute_compiled_tool(
    file_path: &std::path::Path,
    tool_name: &str,
    tool_args: serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    ai_tools::execute_compiled_tool(file_path, tool_name, tool_args)
}

/// Storage for editor instances owned by the plugin
struct EditorStorage {
    panel: Arc<dyn ui::dock::PanelView>,
}

/// The Shader Graph Editor Plugin
pub struct ShaderEditorPlugin {
    /// CRITICAL: Plugin owns ALL editor instances to prevent memory leaks!
    editors: Arc<Mutex<HashMap<usize, EditorStorage>>>,
    next_editor_id: Arc<Mutex<usize>>,
}

impl Default for ShaderEditorPlugin {
    fn default() -> Self {
        Self {
            editors: Arc::new(Mutex::new(HashMap::new())),
            next_editor_id: Arc::new(Mutex::new(0)),
        }
    }
}

impl EditorPlugin for ShaderEditorPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: PluginId::new("com.pulsar.shader-editor"),
            name: "Shader Graph Editor".into(),
            version: "0.1.0".into(),
            author: "Pulsar Team".into(),
            description: "Visual shader graph editor for creating materials".into(),
        }
    }

    fn file_types(&self) -> Vec<FileTypeDefinition> {
        vec![FileTypeDefinition {
            id: FileTypeId::new("material"),
            extension: "material".to_string(),
            display_name: "Material".to_string(),
            icon: ui::IconName::Component,
            color: gpui::rgb(0x4CAF50).into(),
            structure: FileStructure::FolderBased {
                marker_file: "shader_shader_graph_save.json".to_string(),
                template_structure: vec![],
            },
            default_content: json!({
                "format_version": 1,
                "main_graph": {
                    "nodes": {},
                    "connections": [],
                    "metadata": {
                        "name": "MainMaterial",
                        "description": "",
                        "version": "1.0.0",
                        "created_at": "2024-01-01T00:00:00+00:00",
                        "modified_at": "2024-01-01T00:00:00+00:00"
                    },
                    "comments": []
                },
                "shader_metadata": {
                    "shader_model": "Standard_unlit",
                    "stage": "Fragment",
                    "description": "",
                    "category": "Uncategorized"
                }
            }),
            categories: vec!["Shaders".to_string()],
        }]
    }

    fn editors(&self) -> Vec<EditorMetadata> {
        vec![EditorMetadata {
            id: EditorId::new("shader-editor"),
            display_name: "Shader Graph Editor".into(),
            supported_file_types: vec![FileTypeId::new("material")],
        }]
    }

    fn ai_tools(&self) -> Vec<AiToolDefinition> {
        ai_tools::ai_tools()
    }

    fn capabilities_for_file(&self, file_path: &std::path::Path) -> Vec<String> {
        ai_tools::capabilities_for_file(file_path)
    }

    fn execute_ai_tool(
        &self,
        file_path: &std::path::Path,
        tool_name: &str,
        tool_args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        ai_tools::execute_ai_tool(file_path, tool_name, tool_args)
    }

    fn create_editor(
        &self,
        editor_id: EditorId,
        file_path: PathBuf,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Arc<dyn PanelView>, PluginError> {
        log::info!("Creating shader editor with ID: {}", editor_id.as_str());

        if editor_id.as_str() == "shader-editor" {
            let file_path_clone = file_path.clone();

            let panel = cx.new(|cx| {
                match ShaderEditorPanel::new_with_path(file_path_clone.clone(), window, cx) {
                    Ok(p) => {
                        tracing::info!(
                            "create_editor: new_with_path succeeded, graph has {} nodes, current_material_path={:?}",
                            p.graph.nodes.len(),
                            p.current_material_path,
                        );
                        p
                    }
                    Err(e) => {
                        tracing::error!("create_editor: new_with_path FAILED: {}", e);
                        let p = ShaderEditorPanel::new(window, cx);
                        tracing::warn!(
                            "create_editor: fell back to empty panel, graph has {} nodes",
                            p.graph.nodes.len(),
                        );
                        p
                    }
                }
            });

            let graph_snapshot = panel.read(cx).graph.clone();
            tracing::info!(
                "create_editor: AI session snapshot has {} nodes",
                graph_snapshot.nodes.len(),
            );
            ai_tools::upsert_session(file_path.clone(), graph_snapshot);

            let panel_arc: Arc<dyn ui::dock::PanelView> = Arc::new(panel.clone());

            let id = {
                let mut next_id = self.next_editor_id.lock().unwrap();
                let id = *next_id;
                *next_id += 1;
                id
            };

            self.editors.lock().unwrap().insert(
                id,
                EditorStorage {
                    panel: panel_arc.clone(),
                },
            );

            log::info!(
                "Created shader editor instance {} for {:?}",
                id,
                file_path
            );

            Ok(panel_arc)
        } else {
            Err(PluginError::EditorNotFound { editor_id })
        }
    }

    fn on_load(&mut self) {
        crate::features::initialize_features();
        log::info!("Shader Graph Editor Plugin loaded");
    }
}
