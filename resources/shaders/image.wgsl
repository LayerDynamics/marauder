// Image shader: renders textured quads for inline images (Sixel, iTerm2).

struct Uniforms {
    viewport_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    scale_factor: f32,
    _pad: f32,
};

struct ImageInstance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv_rect: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@group(1) @binding(0) var image_texture: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

// Quad vertices (two triangles)
var<private> QUAD_POS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
);

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: ImageInstance,
) -> VertexOutput {
    let quad = QUAD_POS[vertex_index];

    // Instance pos/size are in logical pixels; scale to physical pixels
    // for correct sizing on HiDPI displays.
    let pixel_pos = instance.pos * uniforms.scale_factor + quad * instance.size * uniforms.scale_factor;

    // Convert to NDC
    let ndc = vec2<f32>(
        (pixel_pos.x / uniforms.viewport_size.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / uniforms.viewport_size.y) * 2.0,
    );

    // UV mapping
    let uv = instance.uv_rect.xy + quad * instance.uv_rect.zw;

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(image_texture, image_sampler, in.uv);
    return color;
}
