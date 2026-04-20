// Background-colour fill shader. Used as the back-pass when drawing
// the wireframe overlay, so the mesh dissolves into the viewport
// background and only the wires (drawn afterwards) are visible —
// hiding DC's mixed-winding triangle rendering as a side effect.

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    object_color: vec4<f32>,
    volume_min: vec3<f32>,
    _pad0: f32,
    volume_max: vec3<f32>,
    _pad1: f32,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = uniforms.model * vec4<f32>(in.position, 1.0);
    out.clip_position = uniforms.view_proj * world_pos;
    return out;
}

// Linear value matching the offscreen clear colour in renderer.rs.
// Output is auto-converted to sRGB by the Rgba8UnormSrgb target.
@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.15, 0.15, 0.15, 1.0);
}
