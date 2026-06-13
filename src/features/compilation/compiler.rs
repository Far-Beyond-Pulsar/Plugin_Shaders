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
        let main_tab = self.open_tabs
            .iter()
            .find(|t| t.is_main)
            .unwrap_or(&self.open_tabs[0]);

        self.convert_graph_to_description(&main_tab.graph)
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
