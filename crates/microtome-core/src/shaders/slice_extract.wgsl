// Slice extraction shader.
//
// Samples the intersection texture to determine inside/outside,
// then outputs white for solid regions and black for empty regions.

struct Uniforms {
    cutoff: f32,
    _pad0: f32,
    view_width: f32,
    view_height: f32,
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var i_tex: texture_2d<f32>;

@group(1) @binding(1)
var i_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

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
    let lookup = frag_coord.xy / vec2<f32>(uniforms.view_width, uniforms.view_height);
    let color = textureSample(i_tex, i_sampler, lookup);
    let should_be_white = (color.g - color.r) * (STEPS - 1.0);

    let z_cutoff = 1.0 - uniforms.cutoff;
    if frag_coord.z < z_cutoff {
        discard;
    }

    if front_facing {
        if should_be_white > 0.0 {
            return vec4<f32>(1.0, 1.0, 1.0, 1.0);
        } else {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
    } else {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
}
