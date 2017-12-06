/**
* This shader frag supports structuring elements up to 20x20
* ( pixelRadius = 10) in size, any larger will yield strange results
* Multiple passes can be used instead for erosion/dilation
* of large values.
*
* WebGL uses GSLS 100, so proper dynamic sized loops are not supported.
*
* Should be revisited when WebGL 2.0 comes out with ES 3.0 support
*/

// Image to be dialated/eroded
uniform sampler2D src;

// View dimensions
uniform int viewWidth;
uniform int viewHeight;

// Radius of sampling area
uniform int pixels;
// If == 1, dilate, else erode
uniform int dilate;

float s2f(const in vec4 smpl){
  return (smpl.r + smpl.g + smpl.b) / 3.0;
}

void main(void) {
  int pr2 = pixels * pixels;
  vec2 lookup = gl_FragCoord.xy / vec2(viewWidth, viewHeight );
  float test = s2f(texture2D(src, lookup));
  for(int i = -10; i <= 10; i++ ){
    for(int j = -10; j <= 10; j++ ){
      if( i*i + j*j <= pr2 ){
        vec2 offset = vec2(i,j)/ vec2(viewWidth,viewHeight);
        float s2 = s2f(texture2D(src, lookup + offset));
        if(dilate == 1){
          test = max(test, s2);
        }else{
          test = min(test, s2);
        }
      }
    }
  }
  gl_FragColor = vec4(vec3(test),1);
}  