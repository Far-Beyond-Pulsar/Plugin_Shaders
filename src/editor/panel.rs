//! The shader editor: one document on the shared Graphy canvas.
//!
//! The document is a Graphy `GraphDescription` owned by the canvas entity; there is
//! no other editable copy. Edits are `GraphCommand` batches the canvas applies and
//! reports as `CanvasEvent::Edited`, which is where undo and dirty state hook in.
//! `.material` text is read once at load ([`crate::io::save_load`]) and written from
//! the canvas's graph at save; compiling and the previews read the same graph.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;
use graphy::{GraphCommand, GraphDescription, NodeInstance, Position};
use graphy_gpui::canvas::{system_fonts, CanvasEvent, GraphCanvas};
use graphy_gpui::{Action, GraphProfile as _, MenuItem, MenuTarget};
use graphy_render::Vec2;
use plugin_editor_api::{DocumentId, History, SharedHistory, UndoEntry, UndoService};
use serde_json::Value;
use ui::input::{InputEvent, InputState};
use ui::scroll::ScrollbarState;
use ui::workspace::Workspace;
use ui::VirtualListScrollHandle;
use ui_common::reflected_properties_panel::PropertyStateManager;
use uuid::Uuid;

use crate::actions::CompileShader;
use crate::features::compilation::{CompilationState, CompilationStatus};
use crate::io::formats::{ShaderEditorState, ShaderMetadata};
use crate::previews::PinPreviews;
use crate::profile::ShaderProfile;

/// Grid the canvas snaps nodes and comments to.
pub const GRID: f64 = 10.0;

pub(crate) fn snap(v: f64) -> f64 {
    (v / GRID).round() * GRID
}

/// Mesh types available for the 3D material preview
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshType {
    Sphere,
    Quad,
    Cube,
    Cinderblock,
}

impl MeshType {
    pub fn variants() -> &'static [MeshType] {
        &[MeshType::Sphere, MeshType::Quad, MeshType::Cube, MeshType::Cinderblock]
    }

    pub fn name(&self) -> &'static str {
        match self {
            MeshType::Sphere => "Sphere",
            MeshType::Quad => "Quad",
            MeshType::Cube => "Cube",
            MeshType::Cinderblock => "Cinderblock",
        }
    }
}

/// One undoable edit of the graph: the change set to revert and the commands to
/// replay.
#[derive(Debug, Clone)]
pub struct Command {
    pub label: String,
    pub commands: Vec<GraphCommand>,
    pub changes: graphy::ChangeSet,
}

impl UndoEntry for Command {
    fn label(&self) -> String {
        self.label.clone()
    }
}

/// Compilation history entry
#[derive(Clone, Debug)]
pub struct CompilationHistoryEntry {
    pub timestamp: String,
    pub state: CompilationState,
    pub stage: String,
    pub message: String,
    pub detail: Option<String>,
}

/// What the file holds besides the graph, kept as loaded so a round trip loses
/// nothing.
#[derive(Clone, Debug)]
pub(crate) struct DocMeta {
    pub format_version: u32,
    pub editor_state: Option<ShaderEditorState>,
    pub shader_metadata: ShaderMetadata,
    pub extra: serde_json::Map<String, Value>,
}

impl Default for DocMeta {
    fn default() -> Self {
        Self {
            format_version: crate::io::formats::FORMAT_VERSION,
            editor_state: None,
            shader_metadata: ShaderMetadata::default(),
            extra: serde_json::Map::new(),
        }
    }
}

/// A context menu requested on the canvas.
#[derive(Clone, Debug)]
pub struct CanvasMenu {
    pub at: Point<Pixels>,
    pub local: Vec2,
    pub target: MenuTarget,
    pub items: Vec<MenuItem>,
}

/// The node picker: opened by a right click on empty canvas, or by dropping a wire
/// there (then it offers nodes the wire can reach).
pub struct Palette {
    pub at: Point<Pixels>,
    pub graph_pos: Vec2,
    pub source: Option<graphy::PinRef>,
    pub entries: Vec<graphy_gpui::CatalogEntry>,
    pub focus_pending: bool,
}

/// Main shader editor struct
pub struct ShaderEditorPanel {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) canvas: Entity<GraphCanvas>,
    pub(crate) profile: Arc<ShaderProfile>,
    pub(crate) previews: Arc<PinPreviews>,
    pub(crate) history: SharedHistory<Command>,
    pub(crate) meta: DocMeta,

    // Workspace with full docking support
    pub(super) workspace: Option<Entity<Workspace>>,

    // File I/O
    pub current_material_path: Option<PathBuf>,
    pub tab_title: Option<String>,
    /// Initial graph data is being read on the background executor.
    pub file_loading: bool,
    /// Error from opening the requested material, if loading failed.
    pub load_error: Option<String>,

    // Canvas overlays
    pub(crate) menu: Option<CanvasMenu>,
    pub(crate) palette: Option<Palette>,
    pub(crate) palette_input: Entity<InputState>,
    pub(crate) editing_comment: Option<usize>,
    pub(crate) comment_input: Entity<InputState>,

    // Compilation
    pub compilation_status: CompilationStatus,
    pub compilation_history: Vec<CompilationHistoryEntry>,
    /// Why the material preview's last shader build failed, if it did.
    /// Shown in the Compiler Output panel until a build succeeds.
    pub material_preview_error: Option<String>,
    pub compiler_output_scroll_handle: VirtualListScrollHandle,
    pub compiler_output_scrollbar_state: ScrollbarState,
    pub find_output_scroll_handle: VirtualListScrollHandle,
    pub find_output_scrollbar_state: ScrollbarState,

    /// The host's undo histories (Pulsar-Native#931), once given them.
    pub undo_service: Option<UndoService>,

    // Details panel state
    pub(crate) pin_property_state: PropertyStateManager,
    pub(crate) detail_inputs: std::collections::HashMap<String, Entity<InputState>>,

    // Shader model
    pub shader_model: String,

    // Compiled WGSL output
    pub last_compiled_wgsl: Option<String>,

    // Preview settings
    pub preview_mesh: MeshType,
    pub preview_auto_rotate: bool,
    pub preview_rotation: (f32, f32),

    pub subscriptions: Vec<Subscription>,
}

impl ShaderEditorPanel {
    /// Create a new shader editor panel with the sample graph
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::new_internal(Self::create_sample_graph(), window, cx);
        panel.meta.editor_state = None;
        panel
    }

    /// Create a shader editor panel that loads the material at `file_path`
    /// in the background.
    pub fn new_with_path(
        file_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut panel = Self::new_internal(GraphDescription::new("ShaderGraph"), window, cx);
        panel.file_loading = true;
        panel.set_path(file_path.clone());
        let graph_file = file_path.join(crate::io::save_load::GRAPH_SAVE_FILE_NAME);
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let parsed = cx
                .background_executor()
                .spawn(async move { Self::parse_asset_from_path(&graph_file) })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |panel, cx| {
                    let result = parsed
                        .and_then(|(path, asset)| panel.apply_loaded_asset(path, asset, window, cx));
                    panel.file_loading = false;
                    match result {
                        Ok(()) => {
                            panel.load_error = None;
                            crate::upsert_ai_session(file_path, panel.graph_snapshot(cx));
                        }
                        Err(error) => {
                            tracing::error!("Failed to load shader: {error}");
                            panel.load_error = Some(error);
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();

        Ok(panel)
    }

    /// Create a new shader editor for an engine library (virtual shader)
    pub fn new_for_library(
        library_id: String,
        library_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let _ = library_id;
        let mut panel = Self::new_internal(GraphDescription::new("ShaderGraph"), window, cx);
        panel.tab_title = Some(format!("Library: {}", library_name));
        panel
    }

    /// A graph with a few demo nodes: a shifting rainbow gradient.
    ///
    /// ```text
    ///   frag_uv ──┐
    ///             ├─▶ rainbow ──▶ fragment_output
    ///   time ─────┘
    /// ```
    ///
    /// Every shader graph is a pure dataflow ending in a single output node
    /// (fragment_output/vertex_output): PSGC compiles from that node's inputs, so
    /// every new graph ends with one.
    fn create_sample_graph() -> GraphDescription {
        let definitions = crate::definitions::NodeDefinitions::load();
        let mut graph = GraphDescription::new("ShaderGraph");
        let mut add = |kind: &str, x: f64, y: f64| -> Option<String> {
            let definition = definitions.get_node_definition(kind)?;
            let id = Uuid::new_v4().to_string();
            graph.add_node(crate::profile::node_from_definition(
                definition,
                &id,
                Position::new(x, y),
            ));
            Some(id)
        };
        let frag_uv = add("frag_uv", -400.0, -80.0);
        let time = add("time", -400.0, 80.0);
        let rainbow = add("rainbow", -100.0, 0.0);
        let output = add("fragment_output", 200.0, 0.0);
        let mut connect = |from: &Option<String>, from_pin: &str, to: &Option<String>, to_pin: &str| {
            if let (Some(from), Some(to)) = (from, to) {
                graph.add_connection(graphy::Connection::new(
                    from,
                    from_pin,
                    to,
                    to_pin,
                    graphy::ConnectionType::Data,
                ));
            }
        };
        connect(&frag_uv, "result", &rainbow, "uv");
        connect(&time, "result", &rainbow, "shift");
        connect(&rainbow, "result", &output, "color");
        graph
    }

    fn new_internal(graph: GraphDescription, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.bind_keys([KeyBinding::new("f7", CompileShader, Some("ShaderEditor"))]);

        let previews = Arc::new(PinPreviews::new());
        let profile = Arc::new(ShaderProfile::new(previews.clone()));
        let canvas = cx.new(|cx| {
            GraphCanvas::new(graph, profile.clone(), system_fonts(), cx)
                .expect("the bundled canvas font parses")
        });
        previews.attach(canvas.downgrade(), cx.weak_entity());

        let palette_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search nodes…"));
        let comment_input = cx.new(|cx| InputState::new(window, cx).placeholder("Comment text..."));
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe_in(
            &palette_input,
            window,
            |_this, _input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &comment_input,
            window,
            |this, _input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.finish_comment_editing(cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &canvas,
            window,
            |this, _canvas, event: &CanvasEvent, window, cx| this.on_canvas_event(event, window, cx),
        ));

        let history: SharedHistory<Command> = Arc::new(std::sync::Mutex::new(History::with_capacity(100)));
        let meta = DocMeta::default();
        Self {
            focus_handle: cx.focus_handle(),
            canvas,
            profile,
            previews,
            history,
            shader_model: meta.shader_metadata.shader_model.clone(),
            meta,
            workspace: None,
            current_material_path: None,
            tab_title: None,
            file_loading: false,
            load_error: None,
            menu: None,
            palette: None,
            palette_input,
            editing_comment: None,
            comment_input,
            compilation_status: CompilationStatus::default(),
            compilation_history: Vec::new(),
            material_preview_error: None,
            compiler_output_scroll_handle: VirtualListScrollHandle::new(),
            compiler_output_scrollbar_state: ScrollbarState::default(),
            find_output_scroll_handle: VirtualListScrollHandle::new(),
            find_output_scrollbar_state: ScrollbarState::default(),
            undo_service: None,
            pin_property_state: PropertyStateManager::new(),
            detail_inputs: std::collections::HashMap::new(),
            last_compiled_wgsl: None,
            preview_mesh: MeshType::Sphere,
            preview_auto_rotate: true,
            preview_rotation: (0.0, 0.3),
            subscriptions,
        }
    }

    /// Get focus handle
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    // ── The document ───────────────────────────────────────────────────────

    /// A copy of the document, for compiling, saving and the AI session.
    pub fn graph_snapshot(&self, cx: &App) -> GraphDescription {
        self.canvas.read(cx).graph().clone()
    }

    /// The document for a worker: the same copy, as a `Result` for call sites that
    /// have nothing else to say when it is missing.
    pub(crate) fn document_snapshot(&self, cx: &App) -> Result<GraphDescription, String> {
        Ok(self.graph_snapshot(cx))
    }

    /// Replaces the document (load, reload). Undo history is dropped with it.
    pub(crate) fn install_graph(&mut self, graph: GraphDescription, cx: &mut Context<Self>) {
        self.canvas.update(cx, |canvas, cx| canvas.set_graph(graph, cx));
        {
            let mut history = plugin_editor_api::undo::lock(&self.history);
            history.clear();
            history.mark_saved();
        }
        self.menu = None;
        self.palette = None;
        self.editing_comment = None;
        self.detail_inputs.clear();
        cx.notify();
    }

    pub fn is_dirty(&self) -> bool {
        plugin_editor_api::undo::lock(&self.history).is_dirty()
    }

    // ── Undo ───────────────────────────────────────────────────────────────

    /// Use the host's undo history (Pulsar-Native#931), registered under this
    /// material.
    pub fn set_undo_service(&mut self, service: UndoService, cx: &mut App) {
        let _ = cx;
        self.adopt_history(&service);
        self.undo_service = Some(service);
    }

    pub(crate) fn adopt_history(&self, service: &UndoService) {
        if let Some(path) = &self.current_material_path {
            let material = DocumentId::for_path(path);
            service.adopt(&DocumentId::new(format!("{material}#main")), &self.history);
        }
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let (history, canvas) = (self.history.clone(), self.canvas.clone());
        let undone = plugin_editor_api::undo::lock(&history)
            .undo(|command| {
                canvas.update(cx, |canvas, cx| canvas.undo_changes(&command.changes, cx));
                true
            })
            .is_some();
        if undone {
            self.after_edit(cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let (history, canvas) = (self.history.clone(), self.canvas.clone());
        let redone = plugin_editor_api::undo::lock(&history)
            .redo(|command| {
                canvas
                    .update(cx, |canvas, cx| canvas.redo_commands(&command.commands, cx))
                    .is_ok()
            })
            .is_some();
        if redone {
            self.after_edit(cx);
        }
    }

    fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    // ── Canvas events ──────────────────────────────────────────────────────

    fn on_canvas_event(&mut self, event: &CanvasEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            CanvasEvent::Edited {
                label,
                commands,
                changes,
            } => {
                let mut history = plugin_editor_api::undo::lock(&self.history);
                history.push(Command {
                    label: (*label).to_string(),
                    commands: commands.clone(),
                    changes: changes.clone(),
                });
                history.seal();
                drop(history);
                self.after_edit(cx);
            }
            CanvasEvent::Rejected(why) => tracing::warn!("graph edit rejected: {why}"),
            CanvasEvent::SelectionChanged => cx.notify(),
            CanvasEvent::ViewChanged => {}
            CanvasEvent::Request(action) => self.on_canvas_request(action.clone(), window, cx),
        }
    }

    fn on_canvas_request(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let canvas = self.canvas.clone();
        match action {
            Action::ContextMenu { at, target, items } => {
                self.palette = None;
                let screen = canvas.read(cx).local_to_window(at);
                if let MenuTarget::Canvas { graph_pos } = target {
                    self.open_palette(screen, graph_pos, None, window, cx);
                    self.menu = None;
                } else {
                    self.menu = Some(CanvasMenu {
                        at: screen,
                        local: at,
                        target,
                        items,
                    });
                }
                cx.notify();
            }
            Action::CommentActivated(index) => self.start_comment_editing(index, window, cx),
            Action::ConnectionDropped { from, side, at } => {
                if side != graphy_render::PinSide::Output {
                    return;
                }
                let screen = canvas.read(cx).local_to_window(at);
                let graph_pos = canvas.read(cx).window_to_graph(screen);
                self.menu = None;
                self.open_palette(screen, graph_pos, Some(from), window, cx);
                cx.notify();
            }
            Action::ConnectionRefused(why) => tracing::debug!("connection refused: {why}"),
            _ => {}
        }
    }

    /// Runs the entry of the open context menu.
    pub(crate) fn run_menu_item(&mut self, menu: CanvasMenu, id: &str, cx: &mut Context<Self>) {
        self.menu = None;
        self.canvas.update(cx, |canvas, cx| {
            canvas.run_menu_item(&menu.target, id, menu.local, cx)
        });
        cx.notify();
    }

    // ── Node picker ────────────────────────────────────────────────────────

    fn open_palette(
        &mut self,
        at: Point<Pixels>,
        graph_pos: Vec2,
        source: Option<graphy::PinRef>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profile = self.profile.clone();
        let mut entries = profile.catalog();
        if let Some(source) = &source {
            // Only nodes the dragged wire can reach.
            let graph = self.canvas.read(cx).graph();
            let from_type = graph
                .nodes
                .get(&source.node)
                .and_then(|n| n.outputs.iter().find(|p| p.id == source.pin))
                .map(|p| p.data_type().clone());
            if let Some(from_type) = from_type {
                let resolver = profile.type_resolver();
                entries.retain(|entry| {
                    profile
                        .create_node(&entry.definition, "probe", Position::new(0.0, 0.0))
                        .is_some_and(|node| {
                            node.inputs.iter().any(|pin| {
                                resolver
                                    .compatibility(&from_type, pin.data_type())
                                    .is_connectable()
                            })
                        })
                });
            }
        }
        self.palette_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.palette = Some(Palette {
            at,
            graph_pos,
            source,
            entries,
            focus_pending: true,
        });
    }

    pub(crate) fn close_palette(&mut self, cx: &mut Context<Self>) {
        self.palette = None;
        cx.notify();
    }

    /// Adds the picked node where the picker was opened, wired to the dragged
    /// wire's source if there was one.
    pub(crate) fn place_picked(&mut self, definition: &str, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.take() else {
            return;
        };
        let at = Position::new(
            snap(f64::from(palette.graph_pos.x)),
            snap(f64::from(palette.graph_pos.y)),
        );
        self.add_node(definition, at, palette.source, cx);
        cx.notify();
    }

    /// Adds a node of the catalogue to the graph, wired from `source` when given.
    /// One edit.
    pub(crate) fn add_node(
        &mut self,
        definition: &str,
        at: Position,
        source: Option<graphy::PinRef>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let id = Uuid::new_v4().to_string();
        let node = self.profile.create_node(definition, &id, at)?;
        let mut commands = vec![GraphCommand::AddNode(node.clone())];
        if let Some(source) = source {
            let graph = self.canvas.read(cx).graph();
            commands.extend(connect_commands(&self.profile, graph, &source, &node));
        }
        self.canvas
            .update(cx, |canvas, cx| canvas.apply("Add node", &commands, cx))
            .ok()?;
        Some(id)
    }

    // ── Node properties ────────────────────────────────────────────────────

    /// Sets (or with `None` removes) one property of a node as one edit. No-op when
    /// the value is unchanged.
    pub fn set_node_property(
        &mut self,
        node_id: &str,
        key: &str,
        value: Option<Value>,
        cx: &mut Context<Self>,
    ) {
        let unchanged = self
            .canvas
            .read(cx)
            .graph()
            .nodes
            .get(node_id)
            .is_some_and(|n| n.properties.get(key) == value.as_ref());
        if unchanged {
            return;
        }
        let command = GraphCommand::SetProperty {
            node: node_id.to_string(),
            key: key.to_string(),
            value,
        };
        let _ = self
            .canvas
            .update(cx, |canvas, cx| canvas.apply("Edit property", &[command], cx));
    }

    // ── Comments ───────────────────────────────────────────────────────────

    /// Adds a comment at the centre of the view, around the selected nodes if any.
    pub fn add_comment(&mut self, cx: &mut Context<Self>) {
        let canvas = self.canvas.clone();
        let canvas_ref = canvas.read(cx);
        let (pan, zoom) = canvas_ref.view();
        let vp = canvas_ref.viewport();
        let at = Vec2::new(vp.x * 0.5 / zoom - pan.x, vp.y * 0.5 / zoom - pan.y);
        let mut comment = graphy::core::GraphComment::new(
            "Comment",
            Position::new(snap(f64::from(at.x)), snap(f64::from(at.y))),
            (300.0, 200.0),
        );
        comment.id = Uuid::new_v4().to_string();
        comment.color = [0.2, 0.4, 0.8, 0.35];
        let selected: Vec<String> = canvas_ref.controller().selection.nodes.iter().cloned().collect();
        let rects: Vec<_> = selected
            .iter()
            .filter_map(|id| canvas_ref.scene().node(id))
            .map(|n| n.rect())
            .collect();
        if !rects.is_empty() {
            const PADDING: f32 = 30.0;
            let left = rects.iter().map(|r| r.min.x).fold(f32::INFINITY, f32::min) - PADDING;
            let top = rects.iter().map(|r| r.min.y).fold(f32::INFINITY, f32::min) - PADDING;
            let right = rects.iter().map(|r| r.max.x).fold(f32::NEG_INFINITY, f32::max) + PADDING;
            let bottom = rects.iter().map(|r| r.max.y).fold(f32::NEG_INFINITY, f32::max) + PADDING;
            comment.position = Position::new(snap(f64::from(left)), snap(f64::from(top)));
            comment.size = (
                snap(f64::from(right - left)).max(100.0),
                snap(f64::from(bottom - top)).max(50.0),
            );
            comment.contained_node_ids = selected;
        }
        let index = canvas_ref.graph().comments.len();
        let _ = canvas.update(cx, |canvas, cx| {
            canvas.apply("Add comment", &[GraphCommand::AddComment { index, comment }], cx)
        });
    }

    fn start_comment_editing(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self
            .canvas
            .read(cx)
            .graph()
            .comments
            .get(index)
            .map(|c| c.text.clone())
        else {
            return;
        };
        self.editing_comment = Some(index);
        self.comment_input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        let handle = self.comment_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Renames a comment as one edit.
    pub(crate) fn set_comment_text(&mut self, index: usize, text: String, cx: &mut Context<Self>) {
        let changed = self
            .canvas
            .read(cx)
            .graph()
            .comments
            .get(index)
            .is_some_and(|c| c.text != text);
        if changed {
            let _ = self.canvas.update(cx, |canvas, cx| {
                canvas.apply(
                    "Edit comment",
                    &[GraphCommand::EditComment {
                        index,
                        text: Some(text),
                        color: None,
                        contained_node_ids: None,
                    }],
                    cx,
                )
            });
        }
    }

    fn finish_comment_editing(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.editing_comment.take() else {
            return;
        };
        let text = self.comment_input.read(cx).text().to_string();
        self.set_comment_text(index, text, cx);
        cx.notify();
    }

    // ── Selection helpers ──────────────────────────────────────────────────

    /// Selects `id` in the graph and brings it into view.
    pub(crate) fn focus_node(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.canvas.update(cx, |canvas, cx| {
            canvas.select_nodes([id.clone()], cx);
            canvas.center_on(&id, cx);
        });
    }

    /// Cycles through the catalogue of node ids for tools that need one at a time.
    pub fn node(&self, id: &str, cx: &App) -> Option<NodeInstance> {
        self.canvas.read(cx).graph().nodes.get(id).cloned()
    }
}

/// Commands wiring `source` (an output pin) to the first compatible input of the
/// freshly added `node`.
fn connect_commands(
    profile: &ShaderProfile,
    graph: &GraphDescription,
    source: &graphy::PinRef,
    node: &NodeInstance,
) -> Vec<GraphCommand> {
    let Some(src_pin) = graph
        .nodes
        .get(&source.node)
        .and_then(|n| n.outputs.iter().find(|p| p.id == source.pin))
    else {
        return Vec::new();
    };
    let resolver = profile.type_resolver();
    let Some(target) = node.inputs.iter().find(|p| {
        resolver
            .compatibility(src_pin.data_type(), p.data_type())
            .is_connectable()
    }) else {
        return Vec::new();
    };
    let exec = src_pin.data_type().is_execution();
    let mut commands = Vec::new();
    // An execution output (or a reroute's) drives one wire.
    let single_output = exec
        || graph
            .nodes
            .get(&source.node)
            .is_some_and(|n| n.is_reroute());
    if single_output {
        commands.extend(
            graph
                .connections
                .iter()
                .filter(|c| c.source_node == source.node && c.source_pin == source.pin)
                .cloned()
                .map(GraphCommand::Disconnect),
        );
    }
    commands.push(GraphCommand::Connect(graphy::Connection::new(
        &source.node,
        &source.pin,
        &node.id,
        &target.id,
        if exec {
            graphy::ConnectionType::Execution
        } else {
            graphy::ConnectionType::Data
        },
    )));
    commands
}
