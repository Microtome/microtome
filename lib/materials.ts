import * as THREE from "three";

// bring in frag shaders
import * as erodeDilateShaderFrag from "./shaders/erode_dilate_frag.glsl";
import * as sliceShaderFrag from './shaders/slice_shader_frag.glsl';
import * as orShaderFrag from './shaders/or_shader_frag.glsl';
import * as xorShaderFrag from './shaders/xor_shader_frag.glsl';
import * as depthShaderFrag from './shaders/depth_shader_frag.glsl';
import * as copyShaderFrag from './shaders/copy_shader_frag.glsl';
import * as intersectionShaderFrag from './shaders/intersection_shader_frag.glsl';

export interface UniformValue<T> extends THREE.IUniform {
  type: string,
  value: T
}

export class FloatUniform implements UniformValue<number>{
  type: string = 'f'
  constructor(public value: number) {
  }
}

export class IntegerUniform implements UniformValue<number>{
  type: string = 'i'
  constructor(public value: number) {
  }
}

export class TextureUniform implements UniformValue<THREE.Texture>{
  type: string = 't'
  constructor(public value: THREE.Texture) {
  }
}

export interface ThreeUniforms {
  [uniform: string]: THREE.IUniform
}

export class BaseUniforms implements ThreeUniforms {
  [uniform: string]: THREE.IUniform;
}

export class SliceShaderUniforms extends BaseUniforms {
  constructor(
    public cutoff: FloatUniform,
    // public epsilon: FloatUniform,
    // public dTex: TextureUniform,
    public iTex: TextureUniform,
    public viewWidth: IntegerUniform,
    public viewHeight: IntegerUniform
  ) { super(); }
}

export class IntersectionShaderUniforms extends BaseUniforms {
  constructor(
    public cutoff: FloatUniform
  ) { super(); }
}

export class CopyShaderUniforms extends BaseUniforms {
  constructor(public src: TextureUniform,
    public viewWidth: IntegerUniform,
    public viewHeight: IntegerUniform
  ) { super(); }
}

export class BoolOpShaderUniforms extends BaseUniforms {
  constructor(public src1: TextureUniform,
    public src2: TextureUniform,
    public viewWidth: IntegerUniform,
    public viewHeight: IntegerUniform
  ) { super(); }
}

export class ErodeDialateShaderUniforms extends BaseUniforms {
  constructor(public dilate: IntegerUniform,
    public pixels: IntegerUniform,
    public src: TextureUniform,
    public viewWidth: IntegerUniform,
    public viewHeight: IntegerUniform
  ) { super(); }
}

   let basicVertex = `
void main(void) {
   // compute position
   gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

  export const xLineMaterial: THREE.LineBasicMaterial = new THREE.LineBasicMaterial({ color: 0xd50000, linewidth: 2 });
  export const yLineMaterial: THREE.LineBasicMaterial = new THREE.LineBasicMaterial({ color: 0x00c853, linewidth: 2 });
  export const zLineMaterial: THREE.LineBasicMaterial = new THREE.LineBasicMaterial({ color: 0x2962ff, linewidth: 2 });
  export const bBoxMaterial: THREE.LineBasicMaterial = new THREE.LineBasicMaterial({ color: 0x4fc3f7, linewidth: 2 });
  export const whiteMaterial: THREE.MeshLambertMaterial = new THREE.MeshLambertMaterial({ color: 0xf5f5f5, side: THREE.DoubleSide });
  export const flatWhiteMaterial: THREE.MeshBasicMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff, side: THREE.DoubleSide });
  export const objectMaterial: THREE.MeshPhongMaterial = new THREE.MeshPhongMaterial({ color: 0xcfcfcf, side: THREE.DoubleSide });//, ambient:0xcfcfcf});
  export const selectMaterial: THREE.MeshPhongMaterial = new THREE.MeshPhongMaterial({ color: 0x00cfcf, side: THREE.DoubleSide });//, ambient:0x00cfcf});

  /**
  Material for encoding z depth in image rgba
  */
  export const depthMaterial: THREE.ShaderMaterial = new THREE.ShaderMaterial({
    fragmentShader: depthShaderFrag.default,
    vertexShader: basicVertex,
    blending: THREE.NoBlending
  });

  /**
  Material for alpha rendering object intersections.
  */
  export const intersectionMaterial: THREE.ShaderMaterial = new THREE.ShaderMaterial({
    fragmentShader: intersectionShaderFrag.default,
    vertexShader: basicVertex,
    transparent: true,
    side: THREE.DoubleSide,
    depthWrite: false,
    depthTest: false,
    blending: THREE.AdditiveBlending
    // opacity: 0.1
  });

  /**
  Material for slicing
  */
  export const sliceMaterial: THREE.ShaderMaterial = new THREE.ShaderMaterial({
    fragmentShader: sliceShaderFrag.default,
    vertexShader: basicVertex,
    side: THREE.DoubleSide,
    blending: THREE.NoBlending,
  });
  /**
  Material for erode/dialate
  */
  export const erodeOrDialateMaterial: THREE.ShaderMaterial = new THREE.ShaderMaterial({
    fragmentShader: erodeDilateShaderFrag.default,
    vertexShader: basicVertex,
    side: THREE.DoubleSide,
    blending: THREE.NoBlending,
    uniforms: {}
  });

  /**
  Material for copy
  */
  export const copyMaterial: THREE.ShaderMaterial = new THREE.ShaderMaterial({
    fragmentShader: copyShaderFrag.default,
    vertexShader: basicVertex,
    side: THREE.FrontSide,
    blending: THREE.NoBlending,
    uniforms: {}
  });

  /**
  Material for xor
  */
  export const xorMaterial: THREE.ShaderMaterial = new THREE.ShaderMaterial({
    fragmentShader: xorShaderFrag.default,
    vertexShader: basicVertex,
    side: THREE.FrontSide,
    blending: THREE.AdditiveBlending,
    uniforms: {}
  });

  /**
  Material for or
  */
  export const orMaterial: THREE.ShaderMaterial = new THREE.ShaderMaterial({
    fragmentShader: orShaderFrag.default,
    vertexShader: basicVertex,
    side: THREE.FrontSide,
    blending: THREE.AdditiveBlending,
    uniforms: {}
  });

