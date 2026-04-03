// Phong lighting shader for mesh rendering.
// Ambient + two directional lights (sky from above, ground from below).
// Fragments outside the print volume are tinted red.

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    object_color: vec4<f32>,
    // Print volume bounds (world space)
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
    @location(0) world_normal: vec3<f32>,
    @location(1) world_pos: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = uniforms.model * vec4<f32>(in.position, 1.0);
    out.clip_position = uniforms.view_proj * world_pos;
    out.world_pos = world_pos.xyz;
    let normal_mat = mat3x3<f32>(
        uniforms.model[0].xyz,
        uniforms.model[1].xyz,
        uniforms.model[2].xyz,
    );
    out.world_normal = normalize(normal_mat * in.normal);
    return out;
}

@fragment
fn fs_main(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Flip normal for back-facing fragments (DC output has mixed winding)
    var n = normalize(in.world_normal);
    if (!front_facing) {
        n = -n;
    }

    // Ambient: #777777
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

    var base_color = uniforms.object_color.rgb;

    // Tint red if outside the print volume
    let outside = any(in.world_pos < uniforms.volume_min) ||
                  any(in.world_pos > uniforms.volume_max);
    if outside {
        base_color = vec3<f32>(0.9, 0.15, 0.15);
    }

    let color = base_color * lighting;
    return vec4<f32>(color, uniforms.object_color.a);
}
