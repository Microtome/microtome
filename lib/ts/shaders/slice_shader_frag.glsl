
// current slice height in device coordinates
uniform float cutoff;
// Alpha image sampling which can
// be used to check inside/outside
uniform sampler2D iTex;
uniform int viewWidth;
uniform int viewHeight;

const float STEPS = 256.0;

void main(void) {
  vec2 lookup = gl_FragCoord.xy / vec2(viewWidth, viewHeight );
  vec4 color = texture2D(iTex, lookup);
  float shouldBeWhite = (color.g - color.r) * (STEPS - 1.0);
  float zCutoff = 1.0 - cutoff;
  if ( gl_FragCoord.z < zCutoff ){
    discard;
  }

  if(gl_FrontFacing){
    if(shouldBeWhite > 0.0){
      gl_FragColor = vec4(vec3(1), 1);
    } else {
      gl_FragColor = vec4(0,0,0, 1);
    }
  } else {
    gl_FragColor = vec4(vec3(1),1);
  }
}