// Text pass: instanced textured quads, sample glyph atlas texture × fg_color.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) fg_color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

struct Uniforms {
    viewport_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    _pad: vec2<f32>,
};

struct GlyphInstance {
    @location(0) pos: vec2<f32>,        // pixel position
    @location(1) size: vec2<f32>,       // glyph size in pixels
    @location(2) fg_color: vec4<f32>,
    @location(3) uv_rect: vec4<f32>,    // (u, v, width, height) in atlas
    @location(4) glyph_offset: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var atlas_texture: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

var<private> quad_verts: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
);

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    instance: GlyphInstance,
) -> VertexOutput {
    var out: VertexOutput;

    let local = quad_verts[vi];
    let pixel_pos = instance.pos + instance.glyph_offset + local * instance.size;

    let ndc = vec2<f32>(
        pixel_pos.x / uniforms.viewport_size.x * 2.0 - 1.0,
        1.0 - pixel_pos.y / uniforms.viewport_size.y * 2.0,
    );

    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.fg_color = instance.fg_color;

    // Map local [0,1] to atlas UV rect
    out.uv = instance.uv_rect.xy + local * instance.uv_rect.zw;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;
    return vec4<f32>(in.fg_color.rgb, in.fg_color.a * alpha);
}
