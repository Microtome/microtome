// Intersection test shader.
//
// Renders front faces as red and back faces as green with 1/256 alpha,
// using additive blending to accumulate intersection counts.

struct Uniforms {
    cutoff: f32,
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

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @builtin(front_facing) @interpolate(flat) _dummy: f32,
}

// We can't use front_facing in the vertex shader output,
// so we just output clip_position and use front_facing as a builtin in the fragment shader.

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VsOut {
    var out: VsOut;
    out.clip_position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    return out;
}

const STEPS: f32 = 256.0;

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let z_cutoff = 1.0 - uniforms.cutoff;

    if frag_coord.z < z_cutoff {
        discard;
    }

    if front_facing {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0 / STEPS);
    } else {
        return vec4<f32>(0.0, 1.0, 0.0, 1.0 / STEPS);
    }
}
