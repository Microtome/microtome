declare module '*.html' {
  var _: string;
  export default _;
}

declare module '*.glsl' {
  var _: string;
  export default _;
}

// Make TSC play nice with UMD style globals. UGH
import * as __THREE from 'three';

declare global {
  const THREE: typeof __THREE;
}