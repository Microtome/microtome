// Full-screen blit shader.
// Draws a textured fullscreen triangle to copy an offscreen render target
// onto egui's render pass.

@group(0) @binding(0)
var t_color: texture_2d<f32>;

@group(0) @binding(1)
var t_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Fullscreen triangle: three vertices covering the entire clip space.
// No vertex buffer needed — vertex_index generates the positions.
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var out: VertexOutput;
    // Generate a triangle that covers [-1,1] in clip space:
    //   idx 0 → (-1, -1)  uv (0, 1)
    //   idx 1 → ( 3, -1)  uv (2, 1)
    //   idx 2 → (-1,  3)  uv (0, -1)
    let x = f32(i32(idx & 1u)) * 4.0 - 1.0;
    let y = f32(i32(idx >> 1u)) * 4.0 - 1.0;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    // Map clip coords to UV: x [-1,1] → u [0,1], y [-1,1] → v [1,0]
    out.uv = vec2<f32>((x + 1.0) / 2.0, (1.0 - y) / 2.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_color, t_sampler, in.uv);
}
