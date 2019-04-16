// basic vertex shader providing minimum features required for other shaders.

varying vec3 vNormal;
varying vec3 gNormal;

void main(void) {
   // compute position
   gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
   // Vieport / normals.
   vNormal = normalize( normalMatrix * normal );
   // mesh / global normals.
   // TODO: probably need model view matrix
   // gNormal = normalize(vec3(modelMatrix));
   gNormal = vec3(modelMatrix * vec4(normal, 0.0));
}