// Subpixel antialiased text rendering shader.
// Uses an RGBA atlas where RGB channels contain per-subpixel coverage.
// The final color multiplies the foreground color by the coverage mask.

struct Uniforms {
    viewport_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    scale_factor: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var atlas_tex: texture_2d<f32>;
@group(0) @binding(2) var atlas_samp: sampler;

struct VertexInput {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) fg_color: vec4<f32>,
    @location(3) uv_rect: vec4<f32>,
    @location(4) glyph_offset: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: VertexInput) -> VertexOutput {
    // 6 vertices for a quad (two triangles)
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    let p = positions[vi];

    let pixel_pos = inst.pos + inst.glyph_offset + p * inst.size;
    let ndc = vec2<f32>(
        pixel_pos.x / uniforms.viewport_size.x * 2.0 - 1.0,
        1.0 - pixel_pos.y / uniforms.viewport_size.y * 2.0,
    );

    var out: VertexOutput;
    out.clip_pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = inst.uv_rect.xy + p * inst.uv_rect.zw;
    out.fg_color = inst.fg_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas_tex, atlas_samp, in.uv).rgb;
    let alpha = max(coverage.r, max(coverage.g, coverage.b));
    return vec4<f32>(in.fg_color.rgb * coverage, alpha * in.fg_color.a);
}
