// current slice height in device coordinates
uniform float cutoff;

const float STEPS = 256.0;

void main() {
  float zCutoff = 1.0 - cutoff;

  if ( gl_FragCoord.z < zCutoff ){
    discard;
  }

  if(gl_FrontFacing){
    gl_FragColor = vec4(1.0,0,0,1.0/STEPS);
  }else{
    gl_FragColor = vec4(0,1.0,0,1.0/STEPS);
  }
}