//! Compiler - Compile shader graphs to WGSL code via PSGC

use crate::editor::panel::{ShaderEditorPanel, CompilationHistoryEntry};
use crate::{CompilationState, CompilationStatus};
use gpui::*;

// ── ShaderEditorPanel helpers ──────────────────────────────────────────────

impl ShaderEditorPanel {
    fn push_compilation_history(
        &mut self,
        state: CompilationState,
        stage: impl Into<String>,
        message: impl Into<String>,
        detail: Option<String>,
    ) {
        const MAX_HISTORY_ENTRIES: usize = 2000;

        let now = chrono::Local::now();
        self.compilation_history.push(CompilationHistoryEntry {
            timestamp: now.format("%H:%M:%S").to_string(),
            state,
            stage: stage.into(),
            message: message.into(),
            detail,
        });

        if self.compilation_history.len() > MAX_HISTORY_ENTRIES {
            let overflow = self.compilation_history.len() - MAX_HISTORY_ENTRIES;
            self.compilation_history.drain(0..overflow);
        }
    }

    /// Convert the active graph to a psgc::GraphDescription
    fn convert_graph_to_psgc(&self) -> Result<psgc::GraphDescription, String> {
        use psgc::{Connection, ConnectionType, GraphDescription, NodeInstance, Pin, PinInstance, PinType, Position};

        let mut graph = GraphDescription::new("Shader Graph");
        let main_tab = self.open_tabs
            .iter()
            .find(|t| t.is_main)
            .unwrap_or(&self.open_tabs[0]);

        for bp_node in &main_tab.graph.nodes {
            let mut inputs = Vec::new();
            for pin in &bp_node.inputs {
                inputs.push(PinInstance {
                    id: pin.id.clone(),
                    pin: Pin {
                        name: pin.name.clone(),
                        data_type: psgc::DataType::from_type_str(&pin.data_type.type_name),
                        pin_type: PinType::Input,
                    },
                });
            }

            let mut outputs = Vec::new();
            for pin in &bp_node.outputs {
                outputs.push(PinInstance {
                    id: pin.id.clone(),
                    pin: Pin {
                        name: pin.name.clone(),
                        data_type: psgc::DataType::from_type_str(&pin.data_type.type_name),
                        pin_type: PinType::Output,
                    },
                });
            }

            let mut properties = Vec::new();
            for (k, v) in &bp_node.properties {
                properties.push((k.clone(), v.clone()));
            }

            let node = NodeInstance {
                id: bp_node.id.clone(),
                node_type: bp_node.definition_id.clone(),
                position: Position {
                    x: bp_node.position.x as f64,
                    y: bp_node.position.y as f64,
                },
                inputs,
                outputs,
                properties,
            };

            graph.nodes.insert(bp_node.id.clone(), node);
        }

        for conn in &main_tab.graph.connections {
            let conn_type = match conn.connection_type {
                ui::graph::ConnectionType::Execution => ConnectionType::Execution,
                ui::graph::ConnectionType::Data => ConnectionType::Data,
            };

            graph.connections.push(Connection {
                source_node: conn.source_node.clone(),
                source_pin: conn.source_pin.clone(),
                target_node: conn.target_node.clone(),
                target_pin: conn.target_pin.clone(),
                connection_type: conn_type,
            });
        }

        Ok(graph)
    }

    /// Compile to WGSL via PSGC
    pub fn compile_to_wgsl(&self) -> Result<String, String> {
        let graph = self.convert_graph_to_psgc()?;
        psgc::compile_shader(&graph)
            .map_err(|e| format!("WGSL compilation failed: {}", e))
    }

    /// Dump shader debug info
    fn dump_shader_debug_info(&self, graph: &psgc::GraphDescription) {
        #[derive(serde::Serialize)]
        struct ShaderDebugDump {
            active_tab: String,
            node_count: usize,
            connection_count: usize,
            nodes: Vec<serde_json::Value>,
        }

        let main_tab = self.open_tabs
            .iter()
            .find(|t| t.is_main)
            .unwrap_or(&self.open_tabs[0]);

        let nodes: Vec<serde_json::Value> = graph.nodes.values().map(|n| {
            serde_json::json!({
                "id": n.id,
                "node_type": n.node_type,
            })
        }).collect();

        let dump = ShaderDebugDump {
            active_tab: main_tab.name.clone(),
            node_count: graph.nodes.len(),
            connection_count: graph.connections.len(),
            nodes,
        };

        if let Ok(json) = serde_json::to_string_pretty(&dump) {
            let _ = std::fs::write("shader_graph_debug.json", &json);
        }
    }

    /// Start compilation (called from toolbar)
    pub fn start_compilation(&mut self, cx: &mut Context<Self>) {
        let panel_entity = cx.weak_entity();
        cx.spawn(async move |_entity, mut cx| {
            Self::compile_async(panel_entity, &mut cx).await;
        })
        .detach();
    }

    /// Compile in background with status updates
    pub async fn compile_async(panel_entity: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp) {
        let started_at = std::time::Instant::now();

        let result = panel_entity.update(cx, |panel, cx| {
            panel.compilation_status = CompilationStatus {
                state: CompilationState::Compiling,
                message: "Compiling shader...".to_string(),
                progress: 0.0,
                is_compiling: true,
            };

            panel.push_compilation_history(
                CompilationState::Compiling,
                "prepare",
                "Compilation started",
                Some("Compiling shader graph to WGSL".to_string()),
            );

            panel.sync_all_canvases_to_tabs(cx);
            panel.compile_to_wgsl()
        });

        match result {
            Ok(Ok(wgsl_code)) => {
                smol::Timer::after(std::time::Duration::from_millis(500)).await;
                let _ = panel_entity.update(cx, |panel, cx| {
                    let elapsed_ms = started_at.elapsed().as_millis();

                    panel.last_compiled_wgsl = Some(wgsl_code.clone());

                    panel.compilation_status = CompilationStatus {
                        state: CompilationState::Success,
                        message: "✓ Compilation successful".to_string(),
                        progress: 1.0,
                        is_compiling: false,
                    };

                    panel.push_compilation_history(
                        CompilationState::Success,
                        "complete",
                        "Compilation successful",
                        Some(format!("Duration: {} ms | WGSL output: {} bytes", elapsed_ms, wgsl_code.len())),
                    );

                    cx.notify();
                });
            }
            Ok(Err(e)) => {
                let _ = panel_entity.update(cx, |panel, cx| {
                    let elapsed_ms = started_at.elapsed().as_millis();

                    panel.compilation_status = CompilationStatus {
                        state: CompilationState::Error,
                        message: format!("✗ Compilation failed: {}", e),
                        progress: 0.0,
                        is_compiling: false,
                    };

                    panel.push_compilation_history(
                        CompilationState::Error,
                        "error",
                        "Compilation failed",
                        Some(format!("Duration: {} ms | Reason: {}", elapsed_ms, e)),
                    );

                    cx.notify();
                });
            }
            Err(_) => {
                let _ = panel_entity.update(cx, |panel, cx| {
                    panel.compilation_status = CompilationStatus {
                        state: CompilationState::Error,
                        message: "✗ Compilation failed: panel closed".to_string(),
                        progress: 0.0,
                        is_compiling: false,
                    };
                    panel.push_compilation_history(
                        CompilationState::Error,
                        "error",
                        "Compilation aborted",
                        Some("Editor panel closed before compile completed".to_string()),
                    );
                    cx.notify();
                });
            }
        }

        smol::Timer::after(std::time::Duration::from_secs(3)).await;
        let _ = panel_entity.update(cx, |panel, cx| {
            if panel.compilation_status.state != CompilationState::Compiling {
                panel.compilation_status = CompilationStatus::default();
                cx.notify();
            }
        });
    }
}
