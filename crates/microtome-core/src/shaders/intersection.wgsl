// Intersection test shader.
//
// Renders front faces as red and back faces as green,
// using additive blending to accumulate intersection counts.
// Uses Rgba16Float render target for precise accumulation.

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
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    return out;
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let z_cutoff = 1.0 - uniforms.cutoff;

    if frag_coord.z < z_cutoff {
        discard;
    }

    // Each face contributes 1.0 to its channel.
    // With Rgba16Float and additive blending, this accumulates exactly.
    if front_facing {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    } else {
        return vec4<f32>(0.0, 1.0, 0.0, 1.0);
    }
}
