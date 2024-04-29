// Incoming storage texture
@group(0) @binding(0)
var texture: texture_2d<f32>;

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
        vec2f(-1.0, -3.0),
        vec2f(-1.0, 1.0),
        vec2f(3.0, 1.0),
    );

    var out: VertexOutput;
    out.coord = points[in.vertex_index].xy;
    out.position = vec4f(points[in.vertex_index].xy, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    let uv = vec2f(1.0, -1.0) * in.coord * 0.5 + 0.5;
    let tex_pos = vec2u(uv * vec2f(textureDimensions(texture)));
    return textureLoad(texture, tex_pos, 0);
}