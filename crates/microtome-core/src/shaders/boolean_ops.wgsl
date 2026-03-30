// Boolean operations (OR / XOR) compute shader.
//
// Two entry points operate on two input textures and write to an output texture.

@group(0) @binding(0)
var src1: texture_2d<f32>;

@group(0) @binding(1)
var src2: texture_2d<f32>;

@group(0) @binding(2)
var output_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn cs_or(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(src1);
    if gid.x >= dims.x || gid.y >= dims.y {
        return;
    }

    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    let s1 = textureLoad(src1, coord, 0).r;
    let s2 = textureLoad(src2, coord, 0).r;

    var dst = 0.0;
    if s1 > 0.9 || s2 > 0.9 {
        dst = 1.0;
    }

    textureStore(output_tex, vec2<u32>(gid.x, gid.y), vec4<f32>(dst, dst, dst, 1.0));
}

@compute @workgroup_size(8, 8)
fn cs_xor(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(src1);
    if gid.x >= dims.x || gid.y >= dims.y {
        return;
    }

    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    let s1 = textureLoad(src1, coord, 0).r;
    let s2 = textureLoad(src2, coord, 0).r;

    var dst = 0.0;
    if (s1 > 0.9 || s2 > 0.9) && !(s1 > 0.9 && s2 > 0.9) {
        dst = 1.0;
    }

    textureStore(output_tex, vec2<u32>(gid.x, gid.y), vec4<f32>(dst, dst, dst, 1.0));
}
