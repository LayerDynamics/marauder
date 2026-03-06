// Selection overlay pass: instanced quads with alpha blending.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

struct Uniforms {
    viewport_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    _pad: vec2<f32>,
};

struct SelectionInstance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

// Quad vertices: two triangles forming a unit square
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
    instance: SelectionInstance,
) -> VertexOutput {
    var out: VertexOutput;

    let local = quad_verts[vi];
    let pixel_pos = instance.pos + local * instance.size;

    // Convert pixel coords to NDC: (0,0) top-left -> (-1,1), (w,h) bottom-right -> (1,-1)
    let ndc = vec2<f32>(
        pixel_pos.x / uniforms.viewport_size.x * 2.0 - 1.0,
        1.0 - pixel_pos.y / uniforms.viewport_size.y * 2.0,
    );

    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
