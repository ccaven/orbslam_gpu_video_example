

// Incoming storage texture
@group(0) @binding(0)
var<storage, read> texture: array<u32>;
@group(0) @binding(1)
var<uniform> texture_size: vec2u;
@group(0) @binding(2)
var<uniform> window_size: vec2u;
@group(0) @binding(3)
var base_texture: texture_2d<f32>;


struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) coord: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var points = array(
        // Triangle 1, which overdraws but still covers the entire screen
        vec2f(-1.0, -4.0),
        vec2f(-1.0, 1.0),
        vec2f(4.0, 1.0),
    );

    var out: VertexOutput;
    out.coord = points[in.vertex_index].xy;
    out.position = vec4f(points[in.vertex_index].xy, 0.0, 1.0);

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    // TODO: Correct for size
    let window_aspect = f32(window_size.x) / f32(window_size.y);
    let texture_aspect = f32(texture_size.x) / f32(texture_size.y);

    _ = window_aspect;
    _ = texture_aspect;

    let uv = vec2f(1.0, -1.0) * in.coord * 0.5 + 0.5;

    let tex_pos = vec2u(uv * vec2f(texture_size));

    let tex_index = u32(tex_pos.y * texture_size.x + tex_pos.x);

    let tex_val: u32 = texture[tex_index];

    let tex_val_a = f32((tex_val >> 24) & 255) / 255.0;
    let tex_val_b = f32((tex_val >> 16) & 255) / 255.0;
    let tex_val_g = f32((tex_val >> 8) & 255) / 255.0;
    let tex_val_r = f32((tex_val) & 255) / 255.0;

    let tex_col = vec4f(tex_val_r, tex_val_g, tex_val_b, 0.0);

    return tex_col;
}