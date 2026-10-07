//! Headless thumbnail rendering for saved material assets.

use super::{mesh, renderer::PreviewRenderer};
use crate::io::formats::deserialize_shader;
use image::RgbaImage;
use psgc::{DataType, GraphDescription};
use std::path::Path;

const THUMBNAIL_SIZE: u32 = 128;

/// Render a saved `.material` file using the first frame of its shader on the
/// editor's sphere preview mesh. Registered with `engine_fs` by the plugin.
pub fn render_material_thumbnail(path: &Path) -> Option<RgbaImage> {
    let contents = std::fs::read_to_string(path).ok()?;
    let asset = deserialize_shader(&contents).ok()?;
    let wgsl = compile_material_graph(asset.main_graph)
        .map_err(|error| {
            tracing::warn!(
                "Could not compile material thumbnail for {:?}: {}",
                path,
                error
            );
            error
        })
        .ok()?;
    render_first_frame(&wgsl)
}

fn compile_material_graph(mut graph: GraphDescription) -> Result<String, String> {
    let mut declarations = Vec::new();
    let mut binding = 1u32;
    for node in graph.nodes.values_mut() {
        for input in &mut node.inputs {
            let is_texture_src = match &input.pin.data_type {
                DataType::Data(type_info) => type_info.type_string == "TextureSrc",
                DataType::Exec => false,
            };
            if !is_texture_src {
                continue;
            }
            input.pin.data_type = DataType::typed("texture_2d<f32>");
            if graph.connections.iter().any(|connection| {
                connection.target_node == node.id && connection.target_pin == input.id
            }) {
                continue;
            }

            let path = node
                .properties
                .get(&input.id)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("Texture input '{}' has no selected asset", input.id))?;
            let resource = format!("pulsar_texture_{}", declarations.len());
            node.properties.insert(
                input.id.clone(),
                serde_json::Value::String(resource.clone()),
            );
            declarations.push(format!(
                "// TextureSrc: {}\n@group(1) @binding({binding}) var {resource}: texture_2d<f32>;\n",
                serde_json::to_string(&path).unwrap_or_else(|_| "\"\"".into()),
            ));
            binding += 1;
        }
    }
    if !declarations.is_empty() {
        declarations.insert(
            0,
            "@group(1) @binding(0) var texture_sampler: sampler;\n".into(),
        );
    }
    let shader = psgc::compile_shader(&graph)
        .map_err(|error| format!("WGSL compilation failed: {error}"))?;
    Ok(format!("{}{shader}", declarations.concat()))
}

fn render_first_frame(wgsl: &str) -> Option<RgbaImage> {
    smol::block_on(async { render_first_frame_async(wgsl).await }).ok()
}

async fn render_first_frame_async(wgsl: &str) -> Result<RgbaImage, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|error| format!("No GPU adapter for material thumbnail: {error}"))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("material thumbnail device"),
            ..Default::default()
        })
        .await
        .map_err(|error| format!("Could not create material thumbnail device: {error}"))?;

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8Unorm,
        width: THUMBNAIL_SIZE,
        height: THUMBNAIL_SIZE,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 1,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: Vec::new(),
        color_space: wgpu::SurfaceColorSpace::Auto,
    };

    let mut renderer = PreviewRenderer::new();
    renderer.initialize(&device, &queue, &config);
    renderer.update_shader(wgsl);
    if !renderer.has_material_pipeline() {
        return Err("Material shader did not produce a preview pipeline".into());
    }
    let sphere = mesh::generate_sphere(1.0, 32, 24);
    renderer
        .camera
        .frame_bounding_radius(sphere.bounding_radius());
    renderer.update_mesh(&sphere.vertices, &sphere.indices, sphere.index_count);

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("material thumbnail target"),
        size: wgpu::Extent3d {
            width: THUMBNAIL_SIZE,
            height: THUMBNAIL_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.render_at_time(&target_view, 0.0);

    let bytes_per_row = THUMBNAIL_SIZE * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("material thumbnail readback"),
        size: (bytes_per_row * THUMBNAIL_SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("material thumbnail readback encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(THUMBNAIL_SIZE),
            },
        },
        config_size(&config),
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("Could not wait for material thumbnail: {error}"))?;
    rx.recv()
        .map_err(|error| format!("Material thumbnail readback failed: {error}"))?
        .map_err(|error| format!("Could not map material thumbnail pixels: {error}"))?;

    let mapped = slice
        .get_mapped_range()
        .map_err(|error| format!("Could not read material thumbnail pixels: {error}"))?;
    let mut rgba = Vec::with_capacity((THUMBNAIL_SIZE * THUMBNAIL_SIZE * 4) as usize);
    for pixel in mapped.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    drop(mapped);
    readback.unmap();

    RgbaImage::from_raw(THUMBNAIL_SIZE, THUMBNAIL_SIZE, rgba)
        .ok_or_else(|| "Material thumbnail produced an invalid pixel buffer".to_string())
}

fn config_size(config: &wgpu::SurfaceConfiguration) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: config.width,
        height: config.height,
        depth_or_array_layers: 1,
    }
}
