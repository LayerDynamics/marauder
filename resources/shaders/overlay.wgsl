// Compute overlay shader: renders search highlights, URL underlines, and outlines.
//
// Instance data drives three render modes via the `flags` field:
//   0 = filled highlight (alpha-blended quad)
//   1 = underline (quad squeezed to bottom 2px)
//   2 = outline (fragment shader discards interior pixels)

struct Uniforms {
    viewport_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    scale_factor: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) flags: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) pixel_size: vec2<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: VertexInput,
) -> VertexOutput {
    // 6-vertex fullscreen quad (two triangles)
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );

    let local = positions[vertex_index];

    // For underline mode (flags=1), squeeze the quad to bottom ~2 logical px
    var actual_pos = instance.pos;
    var actual_size = instance.size;
    if (instance.flags == 1u) {
        let thickness = max(round(2.0 * uniforms.scale_factor), 1.0);
        actual_pos.y = instance.pos.y + instance.size.y - thickness;
        actual_size.y = thickness;
    }

    let pixel = actual_pos + local * actual_size;
    let ndc = vec2<f32>(
        pixel.x / uniforms.viewport_size.x * 2.0 - 1.0,
        1.0 - pixel.y / uniforms.viewport_size.y * 2.0,
    );

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = instance.color;
    out.local_uv = local;
    out.flags = instance.flags;
    out.pixel_size = actual_size;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Outline mode (flags=2): discard interior pixels, keep ~1 logical px border
    if (in.flags == 2u) {
        let border = max(round(1.0 * uniforms.scale_factor), 1.0);
        let px = in.local_uv * in.pixel_size;
        if (px.x > border && px.x < in.pixel_size.x - border &&
            px.y > border && px.y < in.pixel_size.y - border) {
            discard;
        }
    }

    return in.color;
}
