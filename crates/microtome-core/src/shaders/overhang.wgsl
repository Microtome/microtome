// Overhang visualization fragment shader.
//
// Blends from white to red based on how much a surface faces downward
// relative to the configured overhang angle threshold.

struct Uniforms {
    cos_angle_rad: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VsOut {
    var out: VsOut;
    out.clip_position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    // Pass through the mesh normal as world normal (no model matrix transform needed
    // since meshes are in world space for slicing).
    out.world_normal = in.normal;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let normal = normalize(in.world_normal);
    let dot_g = dot(normal, vec3<f32>(0.0, 0.0, -1.0));
    let color = (dot_g - uniforms.cos_angle_rad) / (1.0 - uniforms.cos_angle_rad);

    return vec4<f32>(1.0, 1.0 - color, 1.0 - color, 1.0);
}
