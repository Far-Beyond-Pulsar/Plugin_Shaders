//! Core panel struct and initialization
//!
//! This module contains the main `ShaderEditorPanel` struct definition,
//! constructors, and basic accessors.

use gpui::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use ui::{
    input::InputState, resizable::ResizableState,
    scroll::ScrollbarState, VirtualListScrollHandle,
};

use super::tabs::GraphTab;
use crate::core::{events::*, graph::*, types::*};
use crate::editor::workspace_panels::GraphCanvasPanel;
use crate::features::connections::operations::ConnectionDrag;
use crate::ui_components::palette_view::NodePaletteView;
use ui::dock::{DockItem, DockPlacement};

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

/// Main Blueprint Editor Panel struct
pub struct ShaderEditorPanel {
    pub(super) focus_handle: FocusHandle,
    pub graph: BlueprintGraph,

    // Workspace with full docking support
    pub(super) workspace: Option<Entity<ui::workspace::Workspace>>,

    // File I/O
    pub current_material_path: Option<std::path::PathBuf>,
    pub tab_title: Option<String>,

    // Node drag state
    pub dragging_node: Option<String>,
    pub drag_offset: Point<f32>,
    pub initial_drag_positions: HashMap<String, Point<f32>>,
    pub initial_comment_drag_positions: HashMap<String, Point<f32>>,
    pub node_clipboard: Option<BlueprintNode>,
    /// Node click that *may* become a drag once the mouse moves past the threshold.
    /// Set on mouse-down; converted to a real drag in mouse-move.
    pub pending_drag_node: Option<String>,
    /// Canvas-space position where the pending drag mouse-down landed.
    pub pending_drag_start: Option<Point<f32>>,
    /// Pixels of canvas movement required to commit a drag (avoids phantom moves on clicks).
    pub drag_commit_threshold: f32,

    // Connection drag state
    pub dragging_connection: Option<ConnectionDrag>,

    // Panning state
    pub is_panning: bool,
    pub pan_start: Point<f32>,
    pub pan_start_offset: Point<f32>,

    // Selection state
    pub selection_start: Option<Point<f32>>,
    pub selection_end: Option<Point<f32>>,
    pub last_mouse_pos: Option<Point<f32>>,

    // Right-click gesture detection
    pub right_click_start: Option<Point<f32>>,
    pub right_click_threshold: f32,

    // Double-click for reroute nodes
    pub last_click_time: Option<std::time::Instant>,
    pub last_click_pos: Option<Point<f32>>,

    // Coordinate conversion
    /// Window-space origin of the single bp canvas element, captured each frame during paint.
    /// Event handlers subtract this to get canvas-relative (= "screen") coordinates.
    pub canvas_origin: Rc<RefCell<Point<f32>>>,
    pub graph_element_bounds: Option<Bounds<Pixels>>,
    pub graph_element_bounds_by_view: HashMap<String, Bounds<Pixels>>,
    pub interaction_view_id: Option<String>,
    pub interaction_state_by_view: HashMap<String, GraphInteractionState>,

    // Comment system
    pub dragging_comment: Option<String>,
    pub resizing_comment: Option<(String, ResizeHandle)>,
    pub resizing_comment_start: Option<(Point<f32>, Size<f32>)>,
    pub editing_comment: Option<String>,
    pub comment_text_input: Entity<InputState>,
    pub comment_color_bindings_dirty: bool,

    // Subscriptions
    pub subscriptions: Vec<Subscription>,

    // Compilation
    pub compilation_status: CompilationStatus,
    pub compilation_history: Vec<CompilationHistoryEntry>,
    pub compiler_output_scroll_handle: VirtualListScrollHandle,
    pub compiler_output_scrollbar_state: ScrollbarState,
    pub find_output_scroll_handle: VirtualListScrollHandle,
    pub find_output_scrollbar_state: ScrollbarState,

    // Tab system
    pub open_tabs: Vec<GraphTab>,
    pub active_tab_index: usize,
    pub graph_panels: Vec<(String, Entity<GraphCanvasPanel>)>,
    pub graph_workspace_tabs_dirty: bool,

    // Overlay toggles
    pub show_minimap: bool,
    pub show_graph_controls: bool,
    pub wire_active_test_mode: bool,
    pub wire_hidden_test_mode: bool,
    pub running_nodes: HashSet<String>,
    pub graph_anim_start: std::time::Instant,

    // Quick palette overlay (right-click on graph canvas)
    pub popup_palette_graph_pos: Option<Point<f32>>,
    /// Whether the right-click quick-palette overlay is currently visible.
    pub quick_palette_open: bool,
    /// Whether the quick-palette search input should be focused on next paint.
    pub quick_palette_focus_pending: bool,
    /// When opening quick palette from a connection drag, this is the source drag metadata.
    pub quick_palette_connection_source:
        Option<crate::features::connections::operations::ConnectionDrag>,
    /// Window-space position where the user right-clicked (used to anchor the overlay).
    pub quick_palette_screen_pos: Point<Pixels>,
    /// The shared palette view rendered inside the overlay.
    pub quick_palette_view: Entity<NodePaletteView>,

    // Pin hover tooltip state
    pub hovered_pin_tooltip: Option<String>,
    pub hovered_pin_tooltip_pos: Option<Point<Pixels>>,

    // Sidebar tab states
    pub left_tab: usize,    // 0=Compiler, 1=Find
    pub right_tab: usize,       // 0=Details, 1=Palette

    // Tab drag state
    pub dragging_tab: Option<TabDragInfo>,

    pub is_dirty: bool, // Whether there are unsaved changes

    // Undo/redo system
    pub undo_manager: crate::features::undo::UndoManager,

    // ── GPU renderer ──────────────────────────────────────────────────────────
    pub bp_renderer: crate::rendering::gpu::BpRenderer,
    pub bp_surface: Option<gpui::WgpuSurfaceHandle>,

    // ── Context menus (shown as GPUI overlays above the GPU surface) ──────────
    /// Right-clicked node: (node_id, window-space position for anchoring)
    pub node_context_menu: Option<(String, Point<Pixels>)>,
    /// Right-clicked pin: (node_id, pin_id, window-space position)
    pub pin_context_menu: Option<(String, String, Point<Pixels>)>,

    // ── Debugger ──────────────────────────────────────────────────────────────
    /// Set of node IDs that have an active breakpoint.
    pub breakpoints: HashSet<String>,

    // Shader model
    pub shader_model: String,

    // Compiled WGSL output
    pub last_compiled_wgsl: Option<String>,

    // Preview settings
    pub preview_mesh: MeshType,
    pub preview_auto_rotate: bool,
    pub preview_rotation: (f32, f32),
}

/// Information about a tab being dragged
#[derive(Clone, Debug)]
pub struct TabDragInfo {
    pub panel_id: usize,  // Which panel the tab came from
    pub tab_index: usize, // Which tab is being dragged
    pub label: String,
    pub icon: ui::IconName,
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

#[derive(Clone, Debug)]
pub struct GraphInteractionState {
    pub dragging_node: Option<String>,
    pub pending_drag_node: Option<String>,
    pub pending_drag_start: Option<Point<f32>>,
    pub drag_offset: Point<f32>,
    pub initial_drag_positions: HashMap<String, Point<f32>>,
    pub initial_comment_drag_positions: HashMap<String, Point<f32>>,
    pub dragging_connection: Option<ConnectionDrag>,
    pub is_panning: bool,
    pub pan_start: Point<f32>,
    pub pan_start_offset: Point<f32>,
    pub selection_start: Option<Point<f32>>,
    pub selection_end: Option<Point<f32>>,
    pub last_mouse_pos: Option<Point<f32>>,
    pub right_click_start: Option<Point<f32>>,
    pub last_click_time: Option<std::time::Instant>,
    pub last_click_pos: Option<Point<f32>>,
    pub dragging_comment: Option<String>,
    pub resizing_comment: Option<(String, ResizeHandle)>,
    pub resizing_comment_start: Option<(Point<f32>, Size<f32>)>,
    pub editing_comment: Option<String>,
}

impl Default for GraphInteractionState {
    fn default() -> Self {
        Self {
            dragging_node: None,
            pending_drag_node: None,
            pending_drag_start: None,
            drag_offset: Point::new(0.0, 0.0),
            initial_drag_positions: HashMap::new(),
            initial_comment_drag_positions: HashMap::new(),
            dragging_connection: None,
            is_panning: false,
            pan_start: Point::new(0.0, 0.0),
            pan_start_offset: Point::new(0.0, 0.0),
            selection_start: None,
            selection_end: None,
            last_mouse_pos: None,
            right_click_start: None,
            last_click_time: None,
            last_click_pos: None,
            dragging_comment: None,
            resizing_comment: None,
            resizing_comment_start: None,
            editing_comment: None,
        }
    }
}

/// Resize handle for comment boxes
#[derive(Clone, Debug, PartialEq)]
pub enum ResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

impl ShaderEditorPanel {
    /// Create a new blueprint editor panel
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_internal(None, window, cx)
    }

    /// Create a new blueprint editor panel with a file path (for plugin)
    pub fn new_with_path(
        file_path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        tracing::info!(
            ">>> new_with_path: file_path={:?}, graph_file={:?}",
            file_path,
            file_path.join("graph_save.json"),
        );

        let mut panel = Self::new_internal(Some(file_path.clone()), window, cx);
        tracing::info!(
            ">>> new_with_path: after new_internal: open_tabs={}, self.graph.nodes={}, graph_panels={}, current_material_path={:?}",
            panel.open_tabs.len(),
            panel.graph.nodes.len(),
            panel.graph_panels.len(),
            panel.current_material_path,
        );

        // Blueprint classes are folders containing graph_save.json
        let graph_file = file_path.join("graph_save.json");

        // Load the shader file
        if let Err(e) = panel.load_blueprint(graph_file.to_str().unwrap(), window, cx) {
            log::error!("Failed to load shader: {}", e);
            return Err(e.into());
        }

        tracing::info!(
            ">>> new_with_path: loaded. open_tabs={}, self.graph.nodes={}, graph_panels={}, current_material_path={:?}",
            panel.open_tabs.len(),
            panel.graph.nodes.len(),
            panel.graph_panels.len(),
            panel.current_material_path,
        );

        log::info!("Loaded shader from {:?}", file_path);
        Ok(panel)
    }

    /// Create a new shader editor panel with a file to load
    pub fn new_with_file(
        file_path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new_internal(Some(file_path.clone()), window, cx);

        // Try to load the shader file
        if let Err(e) = panel.load_blueprint(file_path.to_str().unwrap(), window, cx) {
            eprintln!("Failed to load shader: {}", e);
        } else {
            println!("Loaded shader from {:?}", file_path);
        }

        panel
    }

    /// Create a new shader editor for an engine library (virtual shader)
    pub fn new_for_library(
        library_id: String,
        library_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new_internal(None, window, cx);
        panel.tab_title = Some(format!("Library: {}", library_name));

        if let Some(main_tab) = panel.open_tabs.get_mut(0) {
            main_tab.name = format!("{} Overview", library_name);
        }

        println!("Created shader editor for library: {}", library_name);
        panel
    }

    /// Create a sample graph with a couple of demo nodes
    fn create_sample_graph() -> BlueprintGraph {
        let mut graph = BlueprintGraph {
            nodes: Vec::new(),
            connections: Vec::new(),
            comments: Vec::new(),
            selected_nodes: Vec::new(),
            selected_comments: Vec::new(),
            zoom_level: 1.0,
            pan_offset: Point::new(0.0, 0.0),
            virtualization_stats: VirtualizationStats::default(),
        };

        // Add a couple of sample nodes using the definitions system
        let definitions = crate::core::definitions::NodeDefinitions::load();
        if let Some(input_category) = definitions.categories.iter().find(|c| c.name == "Input") {
            if let Some(input_def) = input_category.nodes.first() {
                let node = BlueprintNode::from_definition(input_def, Point::new(-200.0, 0.0));
                graph.nodes.push(node);
            }
        }
        if let Some(output_category) = definitions.categories.iter().find(|c| c.name == "Output") {
            if let Some(output_def) = output_category.nodes.first() {
                let node = BlueprintNode::from_definition(output_def, Point::new(200.0, 0.0));
                graph.nodes.push(node);
            }
        }

        graph
    }

    /// Internal constructor with sample graph
    fn new_internal(
        project_path: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let _resizable_state = ResizableState::new(cx);
        let _left_sidebar_resizable_state = ResizableState::new(cx);

        // Create demo graph with sample nodes (only if no file is being loaded)
        let main_graph = if project_path.is_some() {
            // Empty graph - will be loaded from file
            BlueprintGraph {
                nodes: Vec::new(),
                connections: Vec::new(),
                comments: Vec::new(),
                selected_nodes: Vec::new(),
                selected_comments: Vec::new(),
                zoom_level: 1.0,
                pan_offset: Point::new(0.0, 0.0),
                virtualization_stats: VirtualizationStats::default(),
            }
        } else {
            // No file to load - create sample graph
            Self::create_sample_graph()
        };

        let editor_weak = cx.entity().downgrade();
        let quick_palette_view = cx.new(|cx| NodePaletteView::new(editor_weak, window, cx));

        Self {
            focus_handle: cx.focus_handle(),
            graph: main_graph.clone(),
            workspace: None, // Will be initialized in render
            current_material_path: None,
            tab_title: None,
            dragging_node: None,
            drag_offset: Point::new(0.0, 0.0),
            initial_drag_positions: HashMap::new(),
            initial_comment_drag_positions: HashMap::new(),
            node_clipboard: None,
            pending_drag_node: None,
            pending_drag_start: None,
            drag_commit_threshold: 5.0,
            dragging_connection: None,
            is_panning: false,
            pan_start: Point::new(0.0, 0.0),
            pan_start_offset: Point::new(0.0, 0.0),
            selection_start: None,
            selection_end: None,
            last_mouse_pos: None,
            right_click_start: None,
            right_click_threshold: 5.0,
            last_click_time: None,
            last_click_pos: None,
            canvas_origin: Rc::new(RefCell::new(Point::new(0.0, 0.0))),
            graph_element_bounds: None,
            graph_element_bounds_by_view: HashMap::new(),
            interaction_view_id: None,
            interaction_state_by_view: HashMap::new(),
            dragging_comment: None,
            resizing_comment: None,
            resizing_comment_start: None,
            editing_comment: None,
            comment_text_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("Comment text...")),
            comment_color_bindings_dirty: true,
            subscriptions: Vec::new(),
            compilation_status: CompilationStatus::default(),
            compilation_history: Vec::new(),
            compiler_output_scroll_handle: VirtualListScrollHandle::new(),
            compiler_output_scrollbar_state: ScrollbarState::default(),
            find_output_scroll_handle: VirtualListScrollHandle::new(),
            find_output_scrollbar_state: ScrollbarState::default(),
            open_tabs: vec![GraphTab {
                id: "main".to_string(),
                name: "ShaderGraph".to_string(),
                graph: main_graph,
                is_main: true,
                is_dirty: false,
                is_library_macro: false,
                library_id: None,
            }],
            active_tab_index: 0,
            graph_panels: Vec::new(),
            graph_workspace_tabs_dirty: true,
            show_minimap: true,
            show_graph_controls: true,
            wire_active_test_mode: false,
            wire_hidden_test_mode: false,
            running_nodes: HashSet::new(),
            graph_anim_start: std::time::Instant::now(),
            popup_palette_graph_pos: None,
            quick_palette_open: false,
            quick_palette_focus_pending: false,
            quick_palette_connection_source: None,
            quick_palette_screen_pos: Point::default(),
            quick_palette_view,
            hovered_pin_tooltip: None,
            hovered_pin_tooltip_pos: None,
            left_tab: 0,
            right_tab: 0,
            dragging_tab: None,
            is_dirty: false,
            undo_manager: crate::features::undo::UndoManager::new(),
            bp_renderer: crate::rendering::gpu::BpRenderer::new(),
            bp_surface: None,
            node_context_menu: None,
            pin_context_menu: None,
            breakpoints: HashSet::new(),
            shader_model: "Standard_unlit".to_string(),
            last_compiled_wgsl: None,
            preview_mesh: MeshType::Sphere,
            preview_auto_rotate: true,
            preview_rotation: (0.0, 0.3),
        }
    }

    /// Replace the current runtime execution set used by GPU debug rendering.
    pub fn set_running_nodes<I, S>(&mut self, node_ids: I, cx: &mut Context<Self>)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.running_nodes.clear();
        for id in node_ids {
            self.running_nodes.insert(id.as_ref().to_string());
        }
        cx.notify();
    }

    /// Mark/unmark a single node as executing.
    pub fn set_node_running(&mut self, node_id: impl AsRef<str>, running: bool, cx: &mut Context<Self>) {
        if running {
            self.running_nodes.insert(node_id.as_ref().to_string());
        } else {
            self.running_nodes.remove(node_id.as_ref());
        }
        cx.notify();
    }

    /// Clear all runtime execution highlights.
    pub fn clear_running_nodes(&mut self, cx: &mut Context<Self>) {
        self.running_nodes.clear();
        cx.notify();
    }

    /// Get focus handle
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Return the active graph canvas entity, if one exists.
    pub fn active_canvas(&self) -> Option<&Entity<crate::editor::workspace_panels::GraphCanvasPanel>> {
        let tab_id = self.open_tabs.get(self.active_tab_index)?.id.as_str();
        self.graph_panels
            .iter()
            .find(|(id, _)| id == tab_id)
            .map(|(_, entity)| entity)
    }

    fn capture_interaction_state(&self) -> GraphInteractionState {
        GraphInteractionState {
            dragging_node: self.dragging_node.clone(),
            pending_drag_node: self.pending_drag_node.clone(),
            pending_drag_start: self.pending_drag_start,
            drag_offset: self.drag_offset,
            initial_drag_positions: self.initial_drag_positions.clone(),
            initial_comment_drag_positions: self.initial_comment_drag_positions.clone(),
            dragging_connection: self.dragging_connection.clone(),
            is_panning: self.is_panning,
            pan_start: self.pan_start,
            pan_start_offset: self.pan_start_offset,
            selection_start: self.selection_start,
            selection_end: self.selection_end,
            last_mouse_pos: self.last_mouse_pos,
            right_click_start: self.right_click_start,
            last_click_time: self.last_click_time,
            last_click_pos: self.last_click_pos,
            dragging_comment: self.dragging_comment.clone(),
            resizing_comment: self.resizing_comment.clone(),
            resizing_comment_start: self.resizing_comment_start,
            editing_comment: self.editing_comment.clone(),
        }
    }

    fn apply_interaction_state(&mut self, state: GraphInteractionState) {
        self.dragging_node = state.dragging_node;
        self.drag_offset = state.drag_offset;
        self.initial_drag_positions = state.initial_drag_positions;
        self.initial_comment_drag_positions = state.initial_comment_drag_positions;
        self.dragging_connection = state.dragging_connection;
        self.is_panning = state.is_panning;
        self.pan_start = state.pan_start;
        self.pan_start_offset = state.pan_start_offset;
        self.selection_start = state.selection_start;
        self.selection_end = state.selection_end;
        self.last_mouse_pos = state.last_mouse_pos;
        self.right_click_start = state.right_click_start;
        self.last_click_time = state.last_click_time;
        self.last_click_pos = state.last_click_pos;
        self.dragging_comment = state.dragging_comment;
        self.resizing_comment = state.resizing_comment;
        self.resizing_comment_start = state.resizing_comment_start;
        self.editing_comment = state.editing_comment;
    }

    pub(crate) fn activate_interaction_view(&mut self, view_id: &str) {
        self.ensure_active_graph_panel_state(view_id);

        if self.interaction_view_id.as_deref() == Some(view_id) {
            return;
        }

        if let Some(previous_view) = self.interaction_view_id.clone() {
            self.interaction_state_by_view
                .insert(previous_view, self.capture_interaction_state());
        }

        let next_state = self
            .interaction_state_by_view
            .get(view_id)
            .cloned()
            .unwrap_or_default();

        self.apply_interaction_state(next_state);
        self.interaction_view_id = Some(view_id.to_string());
    }

    pub(crate) fn persist_active_interaction_state(&mut self) {
        if let Some(view_id) = self.interaction_view_id.clone() {
            self.interaction_state_by_view
                .insert(view_id, self.capture_interaction_state());
        }
    }

    pub(crate) fn clear_interaction_view_owner(&mut self) {
        self.persist_active_interaction_state();
        self.interaction_view_id = None;
    }

    // ============================================================================
    // Tab Operations
    // ============================================================================

    pub(crate) fn ensure_active_graph_panel_state(&mut self, tab_id: &str) {
        if let Some(tab_index) = self.open_tabs.iter().position(|tab| tab.id == tab_id) {
            if tab_index != self.active_tab_index {
                self.sync_graph_to_active_tab();
                self.active_tab_index = tab_index;
                self.load_active_tab_graph();
            }
        }
    }

    pub(crate) fn refresh_graph_workspace_tabs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.graph_workspace_tabs_dirty {
            return;
        }

        let Some(workspace_entity) = self.workspace.clone() else {
            tracing::info!(
                ">>> refresh_graph_workspace_tabs: workspace is None (initial load before render) — deferring panel creation to render()",
            );
            return;
        };

        tracing::info!(
            ">>> refresh_graph_workspace_tabs: desired tabs={}, current graph_panels={}",
            self.open_tabs.len(),
            self.graph_panels.len(),
        );

        let desired_ids: Vec<String> = self.open_tabs.iter().map(|tab| tab.id.clone()).collect();

        let stale_panels: Vec<Entity<GraphCanvasPanel>> = self
            .graph_panels
            .iter()
            .filter(|(tab_id, _)| !desired_ids.contains(tab_id))
            .map(|(_, panel)| panel.clone())
            .collect();

        for panel in stale_panels {
            workspace_entity.update(cx, |workspace, cx| {
                workspace.remove_panel(panel.clone(), DockPlacement::Center, window, cx);
            });
        }

        self.graph_panels
            .retain(|(tab_id, _)| desired_ids.contains(tab_id));

        let editor_weak = cx.entity().downgrade();
        for tab in &self.open_tabs {
            if self
                .graph_panels
                .iter()
                .any(|(tab_id, _)| tab_id == &tab.id)
            {
                continue;
            }

            let tab_id = tab.id.clone();
            let tab_name = tab.name.clone();
            let tab_is_main = tab.is_main;
            let tab_graph = tab.graph.clone();
            let ew = editor_weak.clone();
            let panel = cx.new(|cx| {
                GraphCanvasPanel::new(ew, tab_id.clone(), tab_name, tab_is_main, tab_graph, window, cx)
            });

            workspace_entity.update(cx, |workspace, cx| {
                workspace.add_panel(panel.clone(), DockPlacement::Center, window, cx);
            });

            self.graph_panels.push((tab.id.clone(), panel));
        }

        self.activate_graph_workspace_tab(self.active_tab_index, window, cx);
        self.graph_workspace_tabs_dirty = false;
    }

    pub(crate) fn activate_graph_workspace_tab(
        &mut self,
        tab_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace_entity) = self.workspace.clone() else {
            return;
        };

        // `tab_index` is a position in `self.open_tabs` / `self.graph_panels` (matched by
        // tab id). The dock's `TabPanel` keeps its own internal panel order, which can
        // diverge from that (e.g. panels are appended to the dock in creation order, not
        // necessarily `open_tabs` order). Resolve the entity for this tab id first, then
        // ask the `TabPanel` for ITS index of that entity — otherwise `set_active_tab`
        // activates whatever panel happens to sit at the same position, leaving the user
        // looking at (and clicking into) a different canvas than `active_canvas()` returns.
        let Some(tab_id) = self.open_tabs.get(tab_index).map(|t| t.id.clone()) else {
            return;
        };
        let Some(panel_entity_id) = self
            .graph_panels
            .iter()
            .find(|(id, _)| id == &tab_id)
            .map(|(_, panel)| panel.entity_id())
        else {
            return;
        };

        workspace_entity.update(cx, |workspace, cx| {
            workspace.dock_area().update(cx, |dock_area, cx| {
                fn activate_tab_item(
                    item: &mut DockItem,
                    panel_entity_id: EntityId,
                    window: &mut Window,
                    cx: &mut App,
                ) -> bool {
                    match item {
                        DockItem::Tabs { view, .. } => {
                            let found = view.update(cx, |tab_panel, cx| {
                                if let Some(ix) =
                                    tab_panel.index_of_panel_by_entity_id(panel_entity_id)
                                {
                                    tab_panel.set_active_tab(ix, window, cx);
                                    true
                                } else {
                                    false
                                }
                            });
                            found
                        }
                        DockItem::Split { items, .. } => {
                            for child in items.iter_mut() {
                                if activate_tab_item(child, panel_entity_id, window, cx) {
                                    return true;
                                }
                            }
                            false
                        }
                        _ => false,
                    }
                }

                let _ = activate_tab_item(dock_area.items_mut(), panel_entity_id, window, cx);
            });
        });
    }

    /// Switch to a different tab, flushing the current canvas first.
    pub fn switch_to_tab(&mut self, tab_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if tab_index < self.open_tabs.len() && tab_index != self.active_tab_index {
            tracing::info!(
                ">>> switch_to_tab: from {} ({} nodes) to {} ({} nodes), graph_panels={}",
                self.active_tab_index,
                self.graph.nodes.len(),
                tab_index,
                self.open_tabs.get(tab_index).map(|t| t.graph.nodes.len()).unwrap_or(0),
                self.graph_panels.len(),
            );

            // Flush the current active canvas into its tab snapshot before leaving.
            let active_tab_id = self.open_tabs.get(self.active_tab_index).map(|t| t.id.clone());
            if let Some(tab_id) = active_tab_id {
                if let Some((_, canvas)) = self.graph_panels.iter().find(|(id, _)| id == &tab_id) {
                    let live = canvas.read(cx).graph.clone();
                    tracing::info!(
                        ">>> switch_to_tab: flushing canvas {} ({} nodes) to tab",
                        tab_id,
                        live.nodes.len(),
                    );
                    self.graph = live.clone();
                    if let Some(tab) = self.open_tabs.get_mut(self.active_tab_index) {
                        tab.graph = live;
                    }
                }
            }
            self.active_tab_index = tab_index;
            // Update self.graph shadow from the new active tab.
            if let Some(tab) = self.open_tabs.get(tab_index) {
                tracing::info!(
                    ">>> switch_to_tab: loading tab {} ({} nodes) into self.graph",
                    tab.id,
                    tab.graph.nodes.len(),
                );
                self.graph = tab.graph.clone();
                self.comment_color_bindings_dirty = true;
            }
            self.activate_graph_workspace_tab(tab_index, window, cx);
            cx.notify();
        }
    }

    /// Open a sub-graph tab by ID, or switch to it if already open
    pub fn open_sub_graph_tab(&mut self, graph_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        tracing::info!(
            ">>> open_sub_graph_tab: graph_id={}, active_tab_index={}, open_tabs={}",
            graph_id,
            self.active_tab_index,
            self.open_tabs.len(),
        );

        if let Some(tab_index) = self.open_tabs.iter().position(|tab| tab.id == graph_id) {
            self.switch_to_tab(tab_index, window, cx);
            return;
        }
    }

    /// Flush the active canvas's live graph into its tab snapshot.
    /// Only call when a canvas exists for the active tab.
    /// Kept for legacy call-sites that run before any canvas is created.
    pub fn sync_graph_to_active_tab(&mut self) {
        if let Some(tab) = self.open_tabs.get_mut(self.active_tab_index) {
            tab.graph = self.graph.clone();
            tab.is_dirty = true;
        }
    }

    /// Flush every open canvas's live graph back into its matching tab snapshot.
    ///
    /// This is the **only** correct sync direction: canvas → tab.
    /// All serialisation paths must call this before reading `open_tabs`.
    pub fn sync_all_canvases_to_tabs(&mut self, cx: &App) {
        let canvas_count = self.graph_panels.len();
        tracing::info!(
            ">>> sync_all_canvases_to_tabs: {} canvas panels, {} open tabs",
            canvas_count,
            self.open_tabs.len(),
        );

        let snapshots: Vec<(String, crate::core::graph::BlueprintGraph)> = self
            .graph_panels
            .iter()
            .map(|(tab_id, canvas)| {
                let g = canvas.read(cx);
                tracing::info!(
                    ">>> sync_all_canvases_to_tabs: reading canvas tab={} nodes={} connections={}",
                    tab_id,
                    g.graph.nodes.len(),
                    g.graph.connections.len(),
                );
                (tab_id.clone(), g.graph.clone())
            })
            .collect();

        for (tab_id, live_graph) in &snapshots {
            if let Some(tab) = self.open_tabs.iter_mut().find(|t| t.id == *tab_id) {
                tracing::info!(
                    ">>> sync_all_canvases_to_tabs: writing to tab={} nodes={} connections={} (was nodes={})",
                    tab_id,
                    live_graph.nodes.len(),
                    live_graph.connections.len(),
                    tab.graph.nodes.len(),
                );
                tab.graph = live_graph.clone();
            } else {
                tracing::warn!(
                    ">>> sync_all_canvases_to_tabs: no matching tab for canvas tab_id={}",
                    tab_id,
                );
            }
        }

        if canvas_count == 0 {
            tracing::info!(
                ">>> sync_all_canvases_to_tabs: NO canvas panels exist — tabs retain their current graph data"
            );
        }
    }

    /// Update `self.graph` shadow from the active tab (or its live canvas if one exists).
    pub fn load_active_tab_graph(&mut self) {
        if let Some(tab) = self.open_tabs.get(self.active_tab_index) {
            self.graph = tab.graph.clone();
            self.comment_color_bindings_dirty = true;
        }
    }

    // ============================================================================
    // Menu Operations
    // ============================================================================

    /// Show node picker at graph position
    pub fn show_node_picker(
        &mut self,
        graph_pos: Point<f32>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Emit event to request node picker from global palette
        cx.emit(ShowNodePickerRequest {
            graph_position: graph_pos,
        });
    }

    // ============================================================================
    // File I/O Operations
    // ============================================================================

    /// Load shader from file.
    pub fn load_blueprint(
        &mut self,
        file_path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let path = std::path::PathBuf::from(file_path);
        tracing::info!(
            ">>> load_blueprint: file_path={:?}, current_material_path={:?}, open_tabs={}, graph_panels={}",
            file_path, self.current_material_path, self.open_tabs.len(), self.graph_panels.len(),
        );

        self.load_from_path(&path, window, cx)?;

        tracing::info!(
            ">>> load_blueprint: after load_from_path: open_tabs={}, self.graph.nodes={}, graph_panels={}, current_material_path={:?}",
            self.open_tabs.len(), self.graph.nodes.len(), self.graph_panels.len(), self.current_material_path,
        );

        Ok(())
    }

    /// Restore tabs from tabs.json (stub — no longer supported)
    fn restore_tabs_state(
        &mut self,
        _material_path: &std::path::Path,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Result<(), String> {
        Ok(())
    }

}
