// Phong lighting shader for mesh rendering.
// Ambient + two directional lights (sky from above, ground from below).

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    object_color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = uniforms.model * vec4<f32>(in.position, 1.0);
    out.clip_position = uniforms.view_proj * world_pos;
    // Transform normal by the upper-left 3x3 of the model matrix.
    // For uniform scale this is sufficient; for non-uniform scale a proper
    // inverse-transpose would be needed, but we keep it simple here.
    let normal_mat = mat3x3<f32>(
        uniforms.model[0].xyz,
        uniforms.model[1].xyz,
        uniforms.model[2].xyz,
    );
    out.world_normal = normalize(normal_mat * in.normal);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);

    // Ambient: #777777 -> rgb(0.467, 0.467, 0.467)
    let ambient_color = vec3<f32>(0.467, 0.467, 0.467);

    // Sky directional light: #AACCFF, intensity 0.65, direction +Z
    let sky_color = vec3<f32>(0.667, 0.8, 1.0);
    let sky_intensity = 0.65;
    let sky_dir = vec3<f32>(0.0, 0.0, 1.0);
    let sky_diffuse = max(dot(n, sky_dir), 0.0);

    // Ground directional light: #775533, intensity 0.45, direction -Z
    let ground_color = vec3<f32>(0.467, 0.333, 0.2);
    let ground_intensity = 0.45;
    let ground_dir = vec3<f32>(0.0, 0.0, -1.0);
    let ground_diffuse = max(dot(n, ground_dir), 0.0);

    let lighting = ambient_color
        + sky_color * sky_intensity * sky_diffuse
        + ground_color * ground_intensity * ground_diffuse;

    let color = uniforms.object_color.rgb * lighting;
    return vec4<f32>(color, uniforms.object_color.a);
}
