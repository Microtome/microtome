/**
 * This module contains slicing related classes.
 */

import * as THREE from "three";

import * as c from "./common";
import * as mats from "./materials";
import * as printer from "./printer";

export const FAR_Z_PADDING: number = 1.0;
export const CAMERA_NEAR: number = 1.0;
export const SLICER_BACKGROUND_Z = -0.1;

// In order for a shell to be fully connected, min pixel
// count is 3 in x or y.
const MIN_SHELL_PIXELS = 3;

interface RenderTargets {
  mask: THREE.WebGLRenderTarget;
  scratch: THREE.WebGLRenderTarget;
  temp1: THREE.WebGLRenderTarget;
  temp2: THREE.WebGLRenderTarget;
  temp3: THREE.WebGLRenderTarget;
}

type TargetName = keyof RenderTargets;

/**
 * Advanced slicer supporting intersecting volumes
 */
export class AdvancedSlicer {

  // Renderer used for slicing
  private renderer: THREE.WebGLRenderer = new THREE.WebGLRenderer({
    alpha: false,
    antialias: false,
    clearColor: 0x000000,
  });

  // Scene containing a single quad for copying data
  // amongst shaders
  private shaderScene: THREE.Scene = new THREE.Scene();

  // Slice camera
  private sliceCamera: THREE.OrthographicCamera = null;
  // z-shell camera
  private zShellCamera: THREE.OrthographicCamera = null;

  /**
   * References to contained targets should not be stored
   * as shaders may swap and reorganize the render targets internally
   *
   * Values should always be accessed by key and not aliased.
   */
  private targets: RenderTargets = {
    mask: null,
    scratch: null,
    temp1: null,
    temp2: null,
    temp3: null,
  };

  // Cache canvas width for dirty checking
  private lastWidth: number = -1;
  // Cache canvas height for dirty checking
  private lastHeight: number = -1;

  // How many pixels do we dialate for rafts
  private raftDilatePixels = 0;

  // If we are shelling, thickness in pixels
  // 0 means disabled, min value is 3
  private shellErodePixels = 0;

  // Materials ---------------------------------------------------------------------------------

  private erodeDialateMaterial = mats.erodeOrDialateMaterial.clone();

  private erodeDialateMaterialUniforms = new mats.ErodeDialateShaderUniforms(
    new mats.IntegerUniform(1),
    new mats.IntegerUniform(0),
    new mats.TextureUniform(null),
    new mats.IntegerUniform(0),
    new mats.IntegerUniform(0));

  private copyMaterial = mats.copyMaterial.clone();

  private copyMaterialUniforms = new mats.CopyShaderUniforms(
    new mats.TextureUniform(null),
    new mats.IntegerUniform(0),
    new mats.IntegerUniform(0));

  private xorMaterial = mats.xorMaterial.clone();
  private xorMaterialUniforms = new mats.BoolOpShaderUniforms(
    new mats.TextureUniform(null),
    new mats.TextureUniform(null),
    new mats.IntegerUniform(0),
    new mats.IntegerUniform(0));

  private orMaterial = mats.orMaterial.clone();
  private orMaterialUniforms = new mats.BoolOpShaderUniforms(
    new mats.TextureUniform(null),
    new mats.TextureUniform(null),
    new mats.IntegerUniform(0),
    new mats.IntegerUniform(0));

  private intersectionTestMaterial = mats.intersectionMaterial.clone();

  private intersectionMaterialUniforms = new mats.IntersectionShaderUniforms(
    new mats.FloatUniform(0),
  );

  private sliceMaterial = mats.sliceMaterial.clone();

  private sliceMaterialUniforms = new mats.SliceShaderUniforms(
    new mats.FloatUniform(0),
    new mats.TextureUniform(null),
    new mats.IntegerUniform(0),
    new mats.IntegerUniform(0));

  constructor(
    private scene: printer.PrinterScene,
    public pixelWidthMM: number,
    public pixelHeightMM: number,
    public raftThicknessMM: number,
    public raftOffset: number,
    public shellInset: number,
    div?: HTMLDivElement) {
    // Can handle printer dimensions of 10x10 meters. :)
    const planeGeom: THREE.PlaneGeometry = new THREE.PlaneGeometry(10000, 10000);
    const planeMaterial = mats.whiteMaterial.clone();
    planeMaterial.side = THREE.DoubleSide;
    this.shaderScene = new THREE.Scene();
    const sliceBackground = new THREE.Mesh(planeGeom, planeMaterial);
    sliceBackground.position.z = SLICER_BACKGROUND_Z;
    this.shaderScene.add(sliceBackground);
    this.sliceCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5);
    this.zShellCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5);
    this.erodeDialateMaterial.uniforms = this.erodeDialateMaterialUniforms;
    this.copyMaterial.uniforms = this.copyMaterialUniforms;
    this.xorMaterial.uniforms = this.xorMaterialUniforms;
    this.orMaterial.uniforms = this.orMaterialUniforms;
    this.intersectionTestMaterial.uniforms = this.intersectionMaterialUniforms;
    this.sliceMaterial.uniforms = this.sliceMaterialUniforms;
    if (!!div) {
      div.innerHTML = "";
      div.appendChild(this.renderer.domElement);
    }
  }

  /**
   * Resize the slicer dimensions
   *
   * This controls the final image size
   *
   * @param width new width
   * @param height new height
   */
  public setSize(width: number, height: number) {
    this.renderer.setSize(width, height);
    const canvas = this.renderer.domElement;
    canvas.width = width;
    canvas.height = height;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
  }

  /**
   * Slice the scene at the given z offset in mm.
   */
  public sliceAt(z: number) {
    this.render(z);
  }

  /**
   * Slice to an image
   * Returns a dataurl of the image
   *
   */
  public sliceAtToImageBase64(z: number): string {
    this.render(z);
    // let gl = this.renderer.context
    // gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, this.dummyReadPixels);
    return this.renderer.domElement.toDataURL("image/png");
  }

  /**
   * Slice to an image
   * Returns a dataurl of the image
   *
   * TODO Promisify
   */
  public sliceAtToBlob(z: number, callback: (blob: Blob) => void): void {
    const gl = this.renderer.context;
    this.render(z);
    // gl.finish();
    // gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, this.dummyReadPixels);
    this.renderer.domElement.toBlob(callback, "image/png");
  }

  private render(z: number) {
    try {
      this.scene.printVolume.visible = false;
      const dirty = this.prepareRender();
      if (z <= this.raftThicknessMM) {
        this.renderRaftSlice();
      } else {
        this.renderSlice(z);
      }
    } finally {
      // Set everything back to normal if stuff goes south
      this.resetScene();
    }
  }

  private renderRaftSlice() {
    // Set model color to white,
    this.scene.overrideMaterial = mats.flatWhiteMaterial;
    this.renderer.render(this.scene, this.sliceCamera, this.targets.temp1, true);
    // Apply dilate filter to texture
    if (this.raftDilatePixels > 0) {
      this.shaderScene.overrideMaterial = this.erodeDialateMaterial;
      this.erodeDialateMaterialUniforms.dilate.value = 1;
      const dilatePixels = this.raftDilatePixels;
      this.erodeOrDilate("temp1", dilatePixels, true);
    }
    // render texture to view
    this.renderSliceFinal("temp1");
  }

  private renderSlice(z: number) {
    this.renderSliceCommon(z);
    this.renderSliceFinal("mask");
  }

  /**
   * Render a slice of scene at z to targets.mask
   */
  private renderSliceCommon(z: number) {
    // Hide print volume
    this.scene.printVolume.visible = false;
    // Hide slice background
    const buildVolHeight = this.scene.printVolume.boundingBox.max.z;
    const sliceZ = (FAR_Z_PADDING + z) / (FAR_Z_PADDING + buildVolHeight);
    // Intersection test material to temp2
    this.scene.overrideMaterial = this.intersectionTestMaterial;
    this.intersectionMaterialUniforms.cutoff.value = sliceZ;
    this.renderer.render(this.scene, this.sliceCamera, this.targets.temp2, true);
    // Render slice to targets.mask
    this.scene.overrideMaterial = this.sliceMaterial;
    this.sliceMaterialUniforms.iTex = new mats.TextureUniform(this.targets.temp2.texture);
    this.sliceMaterialUniforms.cutoff.value = sliceZ;
    this.renderer.render(this.scene, this.sliceCamera, this.targets.mask, true);
  }

  /**
   * Erode or dilate the image in target, putting the final result back in target
   *
   * Utilizes the scratch target for multiple passes
   *
   * @param target the name of the target to erode/dilate
   * @param numPixels the number of pixels to erode or dilate by
   * @param dilate if true dilate, if false erode
   */
  private erodeOrDilate(target: TargetName, numPixels: number, dilate: boolean) {
    // Apply erode/dilate filter to texture
    if (numPixels > 0) {
      this.shaderScene.overrideMaterial = this.erodeDialateMaterial;
      this.erodeDialateMaterialUniforms.dilate.value = dilate ? 1 : 0;
      let dilatePixels = numPixels;
      // Repeatedly apply dilate if needed
      while (dilatePixels > 0) {
        const pixels = dilatePixels % 10 || 10;
        dilatePixels = dilatePixels - pixels;
        this.erodeDialateMaterialUniforms.src = new mats.TextureUniform(this.targets[target].texture);
        this.erodeDialateMaterialUniforms.pixels.value = pixels;
        this.renderer.render(this.shaderScene, this.sliceCamera, this.targets.scratch, true);
        this.swapTargets(target, "scratch");
      }
    }
  }

  /**
   * Display the final slice image stored in srcTarget copying it to the display
   */
  private renderSliceFinal(srcTarget: TargetName) {
    // Render final image
    this.shaderScene.overrideMaterial = this.copyMaterial;
    this.copyMaterialUniforms.src = new mats.TextureUniform(this.targets[srcTarget].texture);
    this.renderer.render(this.shaderScene, this.sliceCamera);
  }

  /**
   * Handle reallocating render targets if dimensions have changed and
   * return true if changes have occured
   */
  private prepareRender(): boolean {
    let dirty = false;
    // Get canvasElement height
    const width = this.renderer.domElement.width;
    const height = this.renderer.domElement.height;
    // If its changed...
    if (width !== this.lastWidth || height !== this.lastHeight) {
      this.lastWidth = width;
      this.lastHeight = height;
      // Recalc pixel width, and shelling parameters
      this.pixelWidthMM = this.scene.printVolume.width / width;
      this.pixelHeightMM = this.scene.printVolume.depth / height;
      if (this.raftOffset && this.raftOffset > 0) {
        this.raftDilatePixels = Math.round(this.raftOffset / this.pixelWidthMM);
      } else {
        this.raftDilatePixels = 0;
      }
      if (this.shellInset && this.shellInset > 0) {
        this.shellErodePixels = Math.round(this.shellInset / this.pixelWidthMM);
        if (this.shellErodePixels < MIN_SHELL_PIXELS) {
          this.shellErodePixels = MIN_SHELL_PIXELS;
        }
      } else {
        this.shellErodePixels = 0;
      }
      this.reallocateTargets(width, height);
      this.prepareCameras(width, height);
      this.prepareShaders(width, height);
      dirty = true;
    }
    return dirty;
  }

  /**
   * Update camera dimensions
   */
  private prepareCameras(newWidth: number, newHeight: number) {
    const pVolumeBBox = this.scene.printVolume.boundingBox;
    const widthRatio: number = Math.abs(pVolumeBBox.max.x - pVolumeBBox.min.x) / newWidth;
    const heightRatio: number = Math.abs(pVolumeBBox.max.y - pVolumeBBox.min.y) / newHeight;
    const scale: number = widthRatio > heightRatio ? widthRatio : heightRatio;
    const right = (scale * newWidth) / 2.0;
    const left = -right;
    const top = (scale * newHeight) / 2.0;
    const bottom = -top;
    const targetZ = this.scene.printVolume.boundingBox.max.z;
    this.sliceCamera.position.z = targetZ + CAMERA_NEAR;
    this.sliceCamera.near = CAMERA_NEAR;
    // We add a little padding to the camera far so that if
    // slice geometry is right on the 0 xy plane, when
    // we draw in the colors and textures we don't get ambiguity
    this.sliceCamera.far = FAR_Z_PADDING + targetZ + CAMERA_NEAR;
    for (const camera of [this.sliceCamera, this.zShellCamera]) {
      camera.right = right;
      camera.left = left;
      camera.top = top;
      camera.bottom = bottom;
      camera.lookAt(c.Z_DOWN);
      camera.up = c.POS_Y;
      camera.updateProjectionMatrix();
    }
  }

  /**
   * Update shader uniforms such as dimensions
   */
  private prepareShaders(newWidth: number, newHeight: number) {
    this.erodeDialateMaterialUniforms.viewWidth.value = newWidth;
    this.erodeDialateMaterialUniforms.viewHeight.value = newHeight;
    this.copyMaterialUniforms.viewWidth.value = newWidth;
    this.copyMaterialUniforms.viewHeight.value = newHeight;
    this.xorMaterialUniforms.viewWidth.value = newWidth;
    this.xorMaterialUniforms.viewHeight.value = newHeight;
    this.orMaterialUniforms.viewWidth.value = newWidth;
    this.orMaterialUniforms.viewHeight.value = newHeight;
    this.sliceMaterialUniforms.viewWidth.value = newWidth;
    this.sliceMaterialUniforms.viewHeight.value = newHeight;
  }

  private resetScene() {
    this.scene.overrideMaterial = null;
    this.scene.printVolume.visible = true;
    this.scene.showPrintObjects();
  }

  private swapTargets(target1: TargetName, target2: TargetName) {
    if (target1 === target2) { throw Error("Targets can not be same!"); }
    const scratch = this.targets[target1];
    this.targets[target1] = this.targets[target2];
    this.targets[target2] = scratch;
  }

  /**
   * reallocate the rendering Targets
   */
  private reallocateTargets(width: number, height: number) {

    const reallocateTarget = (targetName: TargetName) => {
      if (this.targets && this.targets[targetName]) {
        this.targets[targetName].dispose();
      }
      this.targets[targetName] = new THREE.WebGLRenderTarget(width, height, {
        depthBuffer: true,
        format: THREE.RGBAFormat,
        // generateMipMaps: false,
        magFilter: THREE.NearestFilter,
        minFilter: THREE.NearestFilter,
        stencilBuffer: false,
        wrapS: THREE.ClampToEdgeWrapping,
        wrapT: THREE.ClampToEdgeWrapping,
      });
    };

    reallocateTarget("mask");
    reallocateTarget("scratch");
    reallocateTarget("temp1");
    reallocateTarget("temp2");
    reallocateTarget("temp3");
  }
}
