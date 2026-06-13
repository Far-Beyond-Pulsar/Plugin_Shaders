struct GraphUniforms {
    pan: vec2<f32>,
    zoom: f32,
    time: f32,
    viewport: vec2<f32>,
    _pad1: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: GraphUniforms;
@group(1) @binding(0) var preview_tex: texture_2d<f32>;
@group(1) @binding(1) var preview_sampler: sampler;

struct InstanceInput {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
};

@vertex
fn vs_main(instance: InstanceInput, @builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );

    let uv = corners[vertex_index];
    let graph_pos = instance.pos + uv * instance.size;
    let screen = (graph_pos + uniforms.pan) * uniforms.zoom;
    let ndc = vec2<f32>(
        screen.x / uniforms.viewport.x * 2.0 - 1.0,
        1.0 - screen.y / uniforms.viewport.y * 2.0,
    );

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    out.local_uv = uv;
    return out;
}

fn rounded_rect_alpha(uv: vec2<f32>, radius: f32) -> f32 {
    let p = uv - vec2<f32>(0.5, 0.5);
    let half_size = vec2<f32>(0.5 - radius, 0.5 - radius);
    let q = abs(p) - half_size;
    let dist = length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
    return 1.0 - smoothstep(0.0, 0.02, dist);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = rounded_rect_alpha(in.local_uv, 0.075);
    let color = textureSample(preview_tex, preview_sampler, in.uv);
    return vec4<f32>(color.rgb, color.a * alpha);
}
