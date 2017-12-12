// Two sources to xor together
uniform sampler2D src1;
uniform sampler2D src2;

// View dimensions
uniform int viewWidth;
uniform int viewHeight;

void main(void) {
  vec2 lookup = gl_FragCoord.xy / vec2(viewWidth, viewHeight );
  float smpl1 = texture2D(src1, lookup).r;
  float smpl2 = texture2D(src2, lookup).r;
  // No XOR yet? :P
  // float dst = round(smpl1) ^ round(smpl2);
  float dst = 0.0;
  if((smpl1 > 0.9 || smpl2 > 0.9) && !(smpl1 > 0.9 && smpl2 > 0.9)){
    dst = 1.0;
  }
  gl_FragColor = vec4(vec3(dst),1);
}