// Overhang angle.
uniform float angle;

varying vec3 gNormal;

void main(void){ 

  float dotG = dot(gNormal, vec3(0.0,0.0,-1.0));
  float cos30 = 0.5;

    float color = ( dotG - cos30 ) / ( 1.0 - cos30 );
    gl_FragColor=vec4(1.0,1.0-color,1.0-color,1.0);

}