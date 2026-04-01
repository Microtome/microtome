// Slice overlay shader: samples the slice preview texture and renders
// yellow where the slice shows solid (white), transparent elsewhere.
// No depth test — drawn on top of everything in the offscreen pass.

struct OverlayUniforms {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: OverlayUniforms;

@group(1) @binding(0)
var t_slice: texture_2d<f32>;

@group(1) @binding(1)
var t_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_slice, t_sampler, in.uv);
    // Compute brightness from the texture sample
    let brightness = dot(tex_color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    if brightness > 0.5 {
        // Yellow with some transparency
        return vec4<f32>(1.0, 0.9, 0.0, 0.7);
    } else {
        // Fully transparent
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
}
