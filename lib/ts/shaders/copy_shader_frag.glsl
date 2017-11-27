// Texture containing depth info of scene
// packed into rgba
// Must be same size as current viewport
uniform sampler2D src;

// View dimensions
uniform int viewWidth;
uniform int viewHeight;

void main(void) {
  vec2 lookup = gl_FragCoord.xy / vec2(viewWidth, viewHeight );
  gl_FragColor = texture2D(src, lookup);
}