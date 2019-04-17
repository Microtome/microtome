// Overhang angle.
uniform float cosAngleRad;

varying vec3 gNormal;

void main(void){ 

  float dotG = dot(gNormal, vec3(0.0,0.0,-1.0));

    float color = ( dotG - cosAngleRad ) / ( 1.0 - cosAngleRad );
    gl_FragColor=vec4(1.0,1.0-color,1.0-color,1.0);

}