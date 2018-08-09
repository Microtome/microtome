/**
* Properly encoding 32 bit float in rgba from here:
* http://www.gamedev.net/topic/486847-encoding-16-and-32-bit-floating-point-value-into-rgba-byte-texture/
*/
vec4 pack( const in float depth ) {
    const float toFixed = 255.0/256.0;
    vec4 result = vec4(0);
    result.r = fract(depth*toFixed*1.0);
    result.g = fract(depth*toFixed*255.0);
    result.b = fract(depth*toFixed*255.0*255.0);
    result.a = fract(depth*toFixed*255.0*255.0*255.0);
    return result;
}

void main() {
  gl_FragColor = pack(gl_FragCoord.z);
}