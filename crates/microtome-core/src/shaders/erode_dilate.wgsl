// Morphological erode/dilate compute shader.
//
// Uses a circular structuring element of the given radius.
// Erode: output black if any neighbor within the circle is black.
// Dilate: output white if any neighbor within the circle is white.

struct Params {
    radius: i32,
    is_dilate: u32,
}

@group(0) @binding(0)
var<uniform> params: Params;

@group(1) @binding(0)
var input_tex: texture_2d<f32>;

@group(1) @binding(1)
var output_tex: texture_storage_2d<rgba8unorm, write>;

fn sample_brightness(coord: vec2<i32>, dims: vec2<u32>) -> f32 {
    let clamped = clamp(coord, vec2<i32>(0, 0), vec2<i32>(i32(dims.x) - 1, i32(dims.y) - 1));
    let texel = textureLoad(input_tex, clamped, 0);
    return (texel.r + texel.g + texel.b) / 3.0;
}

@compute @workgroup_size(8, 8)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(input_tex);
    if gid.x >= dims.x || gid.y >= dims.y {
        return;
    }

    let center = vec2<i32>(i32(gid.x), i32(gid.y));
    var result = sample_brightness(center, dims);
    let r2 = params.radius * params.radius;

    for (var i = -params.radius; i <= params.radius; i = i + 1) {
        for (var j = -params.radius; j <= params.radius; j = j + 1) {
            if i * i + j * j <= r2 {
                let s = sample_brightness(center + vec2<i32>(i, j), dims);
                if params.is_dilate != 0u {
                    result = max(result, s);
                } else {
                    result = min(result, s);
                }
            }
        }
    }

    textureStore(output_tex, vec2<u32>(gid.x, gid.y), vec4<f32>(result, result, result, 1.0));
}
