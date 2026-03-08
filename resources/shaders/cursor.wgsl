// Cursor overlay: animated block/underline/bar cursor.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

struct CursorUniforms {
    viewport_size: vec2<f32>,
    cursor_pos: vec2<f32>,
    cursor_size: vec2<f32>,
    _pad0: vec2<f32>,
    cursor_color: vec4<f32>,
    time: f32,
    blink_rate: f32,
    _pad1: vec2<f32>,
    _pad2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cursor: CursorUniforms;

var<private> quad_verts: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
);

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var out: VertexOutput;

    let local = quad_verts[vi];
    let pixel_pos = cursor.cursor_pos + local * cursor.cursor_size;

    let ndc = vec2<f32>(
        pixel_pos.x / cursor.viewport_size.x * 2.0 - 1.0,
        1.0 - pixel_pos.y / cursor.viewport_size.y * 2.0,
    );

    out.position = vec4<f32>(ndc, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var blink = 1.0;
    if (cursor.blink_rate > 0.0) {
        blink = step(0.0, sin(cursor.time * cursor.blink_rate * 6.283185));
        blink = max(blink, 0.3); // never fully invisible
    }
    return vec4<f32>(cursor.cursor_color.rgb, cursor.cursor_color.a * blink);
}
