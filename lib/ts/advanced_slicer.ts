import * as core from "./core_threed";

import * as THREE from "three";

// TODO Move to threed?
export const Z_DOWN: THREE.Vector3 = new THREE.Vector3(0, 0, -1000000);
export const Z_UP: THREE.Vector3 = new THREE.Vector3(0, 0, 1000000);
export const ORIGIN: THREE.Vector3 = new THREE.Vector3(0, 0, 0);
export const POS_X: THREE.Vector3 = new THREE.Vector3(1, 0, 0);
export const NEG_X: THREE.Vector3 = new THREE.Vector3(-1, 0, 0);
export const POS_Y: THREE.Vector3 = new THREE.Vector3(0, 1, 0);
export const NEG_Y: THREE.Vector3 = new THREE.Vector3(0, -1, 0);
export const POS_Z: THREE.Vector3 = new THREE.Vector3(0, 0, 1);
export const NEG_Z: THREE.Vector3 = new THREE.Vector3(0, 0, -1);

export const FAR_Z_PADDING: number = 1.0;
export const CAMERA_NEAR: number = 1.0;
export const SLICER_BACKGROUND_Z = -0.1;

interface RenderTargets {
  mask: THREE.WebGLRenderTarget,
  scratch: THREE.WebGLRenderTarget,
  temp1: THREE.WebGLRenderTarget,
  temp2: THREE.WebGLRenderTarget,
  temp3: THREE.WebGLRenderTarget
}

type TargetName = keyof RenderTargets;

/**
Advanced slicer supporting intersecting volumes
*/
export class AdvancedSlicer {

  // Renderer used for slicing
  private renderer: THREE.WebGLRenderer = new THREE.WebGLRenderer({
    alpha: false,
    antialias: false,
    clearColor: 0x000000
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
    temp3: null
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

  // In order for a shell to be fully connected, min pixel
  // count is 3 in x or y.
  private static _MIN_SHELL_PIXELS = 3;

  // Materials ---------------------------------------------------------------------------------

  private erodeDialateMaterial = core.CoreMaterialsFactory.erodeOrDialateMaterial.clone();

  private erodeDialateMaterialUniforms = new core.ErodeDialateShaderUniforms(
    new core.IntegerUniform(1),
    new core.IntegerUniform(0),
    new core.TextureUniform(null),
    new core.IntegerUniform(0),
    new core.IntegerUniform(0));

  private copyMaterial = core.CoreMaterialsFactory.copyMaterial.clone();

  private copyMaterialUniforms = new core.CopyShaderUniforms(
    new core.TextureUniform(null),
    new core.IntegerUniform(0),
    new core.IntegerUniform(0));

  private xorMaterial = core.CoreMaterialsFactory.xorMaterial.clone();
  private xorMaterialUniforms = new core.BoolOpShaderUniforms(
    new core.TextureUniform(null),
    new core.TextureUniform(null),
    new core.IntegerUniform(0),
    new core.IntegerUniform(0));

  private orMaterial = core.CoreMaterialsFactory.orMaterial.clone();
  private orMaterialUniforms = new core.BoolOpShaderUniforms(
    new core.TextureUniform(null),
    new core.TextureUniform(null),
    new core.IntegerUniform(0),
    new core.IntegerUniform(0));

  private intersectionTestMaterial = core.CoreMaterialsFactory.intersectionMaterial.clone();

  private intersectionMaterialUniforms = new core.IntersectionShaderUniforms(
    new core.FloatUniform(0)
  );

  private sliceMaterial = core.CoreMaterialsFactory.sliceMaterial.clone();

  private sliceMaterialUniforms = new core.SliceShaderUniforms(
    new core.FloatUniform(0),
    new core.TextureUniform(null),
    new core.IntegerUniform(0),
    new core.IntegerUniform(0));

  constructor(
    private scene: core.PrinterScene,
    public pixelWidthMM: number,
    public pixelHeightMM: number,
    public raftThicknessMM: number,
    public raftOffset: number,
    public shellInset: number,
    div: HTMLDivElement = undefined) {
    // Can handle printer dimensions of 10x10 meters. :)
    var planeGeom: THREE.PlaneGeometry = new THREE.PlaneGeometry(10000, 10000);
    var planeMaterial = core.CoreMaterialsFactory.whiteMaterial.clone();
    planeMaterial.side = THREE.DoubleSide;
    this.shaderScene = new THREE.Scene();
    let sliceBackground = new THREE.Mesh(planeGeom, planeMaterial);
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
  setSize(width: number, height: number) {
    this.renderer.setSize(width, height);
    let canvas = this.renderer.domElement;
    canvas.width = width;
    canvas.height = height;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
  }

  /**
  * Slice the scene at the given z offset in mm.
  */
  sliceAt(z: number) {
    this.render(z);
  }

  /***
  * Slice to an image
  * Returns a dataurl of the image
  */
  sliceAtToImageBase64(z: number): String {
    this.render(z);
    // let gl = this.renderer.context
    // gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, this.dummyReadPixels);
    return this.renderer.domElement.toDataURL("image/png");
  }

  /***
  * Slice to an image
  * Returns a dataurl of the image
  */
  sliceAtToBlob(z: number, callback: (blob: Blob) => void): void {
    this.render(z);
    // let gl = this.renderer.context
    // gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, this.dummyReadPixels);
    this.renderer.domElement.toBlob(callback, "image/png");
  }

  private render(z: number) {
    try {
      this.scene.printVolume.visible = false;
      let dirty = this.prepareRender();
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
    this.scene.overrideMaterial = core.CoreMaterialsFactory.flatWhiteMaterial;
    this.renderer.render(this.scene, this.sliceCamera, this.targets.temp1, true);
    // Apply dilate filter to texture
    if (this.raftDilatePixels > 0) {
      this.shaderScene.overrideMaterial = this.erodeDialateMaterial;
      this.erodeDialateMaterialUniforms.dilate.value = 1;
      let dilatePixels = this.raftDilatePixels;
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
    this.scene.printVolume.visible = false
    // Hide slice background
    let buildVolHeight = this.scene.printVolume.boundingBox.max.z;
    let sliceZ = (FAR_Z_PADDING + z) / (FAR_Z_PADDING + buildVolHeight);
    // Intersection test material to temp2
    this.scene.overrideMaterial = this.intersectionTestMaterial;
    this.intersectionMaterialUniforms.cutoff.value = sliceZ;
    this.intersectionTestMaterial.needsUpdate = true;
    this.renderer.render(this.scene, this.sliceCamera, this.targets.temp2, true);
    // Render slice to targets.mask
    this.scene.overrideMaterial = this.sliceMaterial;
    this.sliceMaterialUniforms.iTex = new core.TextureUniform(this.targets.temp2.texture);
    this.sliceMaterialUniforms.cutoff.value = sliceZ;
    this.sliceMaterial.needsUpdate = true;
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
        let pixels = dilatePixels % 10 || 10;
        dilatePixels = dilatePixels - pixels;
        this.erodeDialateMaterialUniforms.src = new core.TextureUniform(this.targets[target].texture);
        this.erodeDialateMaterialUniforms.pixels.value = pixels;
        this.erodeDialateMaterial.needsUpdate = true;
        this.renderer.render(this.shaderScene, this.sliceCamera, this.targets.scratch, true);
        this.swapTargets(target,"scratch");
      }
    }
  }

  /**
  * Display the final slice image stored in srcTarget copying it to the display
  */
  private renderSliceFinal(srcTarget: TargetName) {
    // Render final image
    this.shaderScene.overrideMaterial = this.copyMaterial;
    this.copyMaterialUniforms.src = new core.TextureUniform(this.targets[srcTarget].texture);
    this.copyMaterial.needsUpdate = true;
    this.renderer.render(this.shaderScene, this.sliceCamera);
  }


  /**
  * Handle reallocating render targets if dimensions have changed and
  * return true if changes have occured
  */
  private prepareRender(): boolean {
    let dirty = false;
    // Get canvasElement height
    let width = this.renderer.domElement.width;
    let height = this.renderer.domElement.height;
    // If its changed...
    if (width != this.lastWidth || height != this.lastHeight) {
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
        if (this.shellErodePixels < AdvancedSlicer._MIN_SHELL_PIXELS) {
          this.shellErodePixels = AdvancedSlicer._MIN_SHELL_PIXELS;
        }
      } else {
        this.shellErodePixels = 0;
      }
      this.reallocateTargets(width, height);
      this.prepareCameras(width, height);
      this.prepareShaders(width, height);
      this.lastWidth = width;
      this.lastHeight = height;
      dirty = true;
    }
    return dirty;
  }

  /**
  * Update camera dimensions
  */
  private prepareCameras(newWidth: number, newHeight: number) {
    var pVolumeBBox = this.scene.printVolume.boundingBox;
    var widthRatio: number = Math.abs(pVolumeBBox.max.x - pVolumeBBox.min.x) / newWidth;
    var heightRatio: number = Math.abs(pVolumeBBox.max.y - pVolumeBBox.min.y) / newHeight;
    var scale: number = widthRatio > heightRatio ? widthRatio : heightRatio;
    let right = (scale * newWidth) / 2.0;
    let left = -right;
    let top = (scale * newHeight) / 2.0;
    let bottom = -top
    let targetZ = this.scene.printVolume.boundingBox.max.z;
    this.sliceCamera.position.z = targetZ + CAMERA_NEAR;
    this.sliceCamera.near = CAMERA_NEAR;
    // We add a little padding to the camera far so that if
    // slice geometry is right on the 0 xy plane, when
    // we draw in the colors and textures we don't get ambiguity
    this.sliceCamera.far = FAR_Z_PADDING + targetZ + CAMERA_NEAR;
    for (let camera of [this.sliceCamera, this.zShellCamera]) {
      camera.right = right;
      camera.left = left;
      camera.top = top;
      camera.bottom = bottom;
      camera.lookAt(Z_DOWN);
      camera.up = POS_Y;
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

  private swapTargets(target1: TargetName, target2: TargetName){
    if (target1 === target2) throw Error ("Targets can not be same!");
    let scratch = this.targets[target1];
    this.targets[target1] = this.targets[target2];
    this.targets[target2] = scratch;
  }

  /**
  * reallocate the rendering Targets
  */
  private reallocateTargets(width: number, height: number) {

    let reallocateTarget = (targetName: TargetName, width: number, height: number) => {
      this.targets[targetName] && this.targets[targetName].dispose();
      this.targets[targetName] = new THREE.WebGLRenderTarget(width, height, {
        format: THREE.RGBAFormat,
        depthBuffer: true,
        stencilBuffer: false,
        // generateMipMaps: false,
        minFilter: THREE.NearestFilter,
        magFilter: THREE.NearestFilter,
        wrapS: THREE.ClampToEdgeWrapping,
        wrapT: THREE.ClampToEdgeWrapping
      });
    }

    reallocateTarget("mask", width, height);
    reallocateTarget("scratch", width, height);
    reallocateTarget("temp1", width, height);
    reallocateTarget("temp2", width, height);
    reallocateTarget("temp3", width, height);
  }
}
