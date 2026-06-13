//! Dockable material preview panel

use gpui::*;
use ui::dock::{Panel, PanelEvent};
use ui::{
    button::{Button, ButtonVariants as _},
    h_flex, v_flex, ActiveTheme, IconName,
};

use super::mesh::{self, PreviewMeshData};
use super::renderer::PreviewRenderer;
use crate::editor::panel::MeshType;

pub struct MaterialPreviewPanel {
    editor: WeakEntity<crate::editor::panel::ShaderEditorPanel>,
    focus_handle: FocusHandle,
    renderer: PreviewRenderer,
    pub current_mesh: MeshType,
    pub auto_rotate: bool,
    pub auto_rotate_speed: f32,
    surface_handle: Option<gpui::WgpuSurfaceHandle>,
    needs_rebuild: bool,
    last_shader_source: Option<String>,
    subscriptions: Vec<Subscription>,
}

impl MaterialPreviewPanel {
    pub fn new(
        editor: WeakEntity<crate::editor::panel::ShaderEditorPanel>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            editor,
            focus_handle: cx.focus_handle(),
            renderer: PreviewRenderer::new(),
            current_mesh: MeshType::Sphere,
            auto_rotate: true,
            auto_rotate_speed: 0.5,
            surface_handle: None,
            needs_rebuild: true,
            last_shader_source: None,
            subscriptions: Vec::new(),
        }
    }

    pub fn rebuild_surface(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.needs_rebuild {
            return;
        }

        let Some(device) = self.renderer.device.clone() else { return };
        let Some(queue) = self.renderer.queue.clone() else { return };

        let size = window.bounds().size;
        let width = (size.width.to_f64() as u32).max(1);
        let height = (size.height.to_f64() as u32).max(1);

        let Some(surface) = window.create_wgpu_surface(width, height, wgpu::TextureFormat::Bgra8Unorm) else {
            return;
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8Unorm,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };

        self.renderer.initialize(&device, &queue, &config);
        self.renderer.update_camera(width as f32 / height as f32);

        self.surface_handle = Some(surface);
        self.needs_rebuild = false;
    }

    pub fn update_mesh(&mut self, mesh_type: MeshType) {
        self.current_mesh = mesh_type;
        let mesh_data = generate_mesh_data(mesh_type);
        self.renderer
            .update_mesh(&mesh_data.vertices, &mesh_data.indices, mesh_data.index_count);
        self.needs_rebuild = true;
    }

    pub fn update_shader(&mut self, wgsl_source: &str) {
        if self.last_shader_source.as_deref() == Some(wgsl_source) {
            return;
        }
        self.last_shader_source = Some(wgsl_source.to_string());
        self.renderer.update_shader(wgsl_source);
    }

    fn render_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if self.needs_rebuild {
            self.rebuild_surface(window, cx);
        }

        if self.auto_rotate {
            if let Some(editor) = self.editor.upgrade() {
                let (yaw, pitch) = editor.read(cx).preview_rotation;
                let new_yaw = yaw + self.auto_rotate_speed * 0.016;
                editor.update(cx, |panel, _cx| {
                    panel.preview_rotation = (new_yaw, pitch);
                });
            }
        }

        if let Some(editor) = self.editor.upgrade() {
            let (yaw, pitch) = editor.read(cx).preview_rotation;
            self.renderer.camera.yaw = yaw;
            self.renderer.camera.pitch = pitch;
        }

        let wgsl_to_compile: Option<String> = self.editor.upgrade()
            .and_then(|editor| editor.read(cx).last_compiled_wgsl.clone());

        if let Some(ref wgsl) = wgsl_to_compile {
            if self.renderer.device.is_some() && self.renderer.queue.is_some() {
                self.update_shader(wgsl);
            }
        }

        if let Some(surface) = &self.surface_handle {
            if let Some(view) = surface.back_buffer_view() {
                self.renderer.render(&view);
                surface.present();
            }
        }

        div()
            .size_full()
            .bg(gpui::rgb(0x1a1a1a))
            .into_any_element()
    }
}

fn generate_mesh_data(mesh_type: MeshType) -> PreviewMeshData {
    match mesh_type {
        MeshType::Sphere => mesh::generate_sphere(1.0, 32, 24),
        MeshType::Quad => mesh::generate_quad(),
        MeshType::Cube => mesh::generate_cube(),
        MeshType::Cinderblock => mesh::generate_cinderblock(),
    }
}

impl EventEmitter<PanelEvent> for MaterialPreviewPanel {}

impl Render for MaterialPreviewPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(self.render_preview(window, cx))
    }
}

impl Focusable for MaterialPreviewPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for MaterialPreviewPanel {
    fn panel_name(&self) -> &'static str {
        "material-preview"
    }

    fn title(&self, _window: &Window, _cx: &App) -> AnyElement {
        "Material Preview".into_any_element()
    }
}
