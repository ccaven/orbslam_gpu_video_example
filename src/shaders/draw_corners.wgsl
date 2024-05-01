@group(0) @binding(0)
var<uniform> base_resolution: vec2u;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) texcoord: vec2<f32>,
    @location(1) scale: f32
};

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) corner_x: u32,
    @location(1) corner_y: u32,
    @location(2) corner_angle: f32,
    @location(3) corner_octave: u32
) -> VertexOutput {
    var output: VertexOutput;

    var points = array(
        vec2f(-1.0, -1.0),
        vec2f(1.0, 1.0),
        vec2f(1.0, -1.0),
        vec2f(-1.0, -1.0),
        vec2f(1.0, 1.0),
        vec2f(-1.0, 1.0),
    );

    let pos = points[vertex_index];

    // TODO: Compute position, size, and rotation of corner

    let corner_size = 0.01;
    let scaled_pos = pos * f32(1u << corner_octave);

    let rotation_matrix = mat2x2f(
        cos(corner_angle), sin(corner_angle),
        -sin(corner_angle), cos(corner_angle) 
    );

    let rotated_pos = rotation_matrix * scaled_pos;
    
    let aspect_corrected = rotated_pos / vec2f(base_resolution.xy) * f32(base_resolution.x);

    let translation = vec2f(
        f32(corner_x * (1u << corner_octave)),
        f32(corner_y * (1u << corner_octave))
    ) / vec2f(base_resolution) * 2.0 - 1.0;

    let z = 1.0 - 0.1 * f32(corner_octave);

    output.position = vec4f(corner_size * aspect_corrected + translation, 0.0, 1.0);
    output.texcoord = pos;
    output.scale = f32(1u << corner_octave);

    return output;
}

@fragment
fn fs_main(
    in: VertexOutput
) -> @location(0) vec4f {

    // TODO: Draw circle
    let m = dot(in.texcoord, in.texcoord);

    let mid = 0.8;
    let thickness = 0.1 / in.scale;

    let min_mag = mid - thickness;
    let max_mag = mid + thickness;

    if m < min_mag * min_mag || m > max_mag * max_mag {
        discard;
    }

    return vec4f(in.texcoord * 0.5 + 0.5, 0.0, 1.0);
}