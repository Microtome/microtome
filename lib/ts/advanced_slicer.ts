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

/**
Advanced slicer supporting shelling, support pattern generation,
hollow shells, etc.
*/
export class AdvancedSlicer {

  // Renderer used for slicing
  private renderer: THREE.WebGLRenderer = new THREE.WebGLRenderer({ alpha: false, antialias: false, clearColor: 0x000000 });

  // Slice camera
  private sliceCamera: THREE.OrthographicCamera = null;
  // z-shell camera
  private zShellCamera: THREE.OrthographicCamera = null;

  // Final composition target where final image is composed
  private finalCompositeTarget: THREE.WebGLRenderTarget = null;

  // Because of how we slice, we can't use stencil buffer
  private maskTarget: THREE.WebGLRenderTarget = null;

  // // Depth target
  // private depthTarget: THREE.WebGLRenderTarget = null;

  // Temp target 1
  private tempTarget1: THREE.WebGLRenderTarget = null;

  // Temp target 2
  private tempTarget2: THREE.WebGLRenderTarget = null;

  // Temp target 2
  private zshellTarget: THREE.WebGLRenderTarget = null;

  // Cache canvas width for dirty checking
  private lastWidth: number = -1;
  // Cache canvas height for dirty checking
  private lastHeight: number = -1;

  // In order for a shell to be fully connected, min pixel
  // count is 3 in x or y.
  private static _MIN_SHELL_PIXELS = 3;

  private sliceBackground: THREE.Mesh;

  private raftDilatePixels = 0;

  private shellErodePixels = 0;

  // private dummyReadPixels = new Uint8Array(4);

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

  /**
  *
  */
  constructor(
    private scene: core.PrinterScene,
    public pixelWidthMM: number,
    public pixelHeightMM: number,
    public raftThicknessMM: number,
    public raftOffset: number,
    public shellInset: number) {
    var planeGeom: THREE.PlaneGeometry = new THREE.PlaneGeometry(1.0, 1.0);
    var planeMaterial = core.CoreMaterialsFactory.whiteMaterial.clone();
    planeMaterial.side = THREE.DoubleSide;
    this.sliceBackground = new THREE.Mesh(planeGeom, planeMaterial);
    this.sliceBackground.position.z = SLICER_BACKGROUND_Z;
    this.sliceBackground.visible = false;
    this.scene.add(this.sliceBackground);
    this.sliceCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5);
    this.zShellCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5);
    this.erodeDialateMaterial.uniforms = this.erodeDialateMaterialUniforms;
    this.copyMaterial.uniforms = this.copyMaterialUniforms;
    this.xorMaterial.uniforms = this.xorMaterialUniforms;
    this.orMaterial.uniforms = this.orMaterialUniforms;
    this.intersectionTestMaterial.uniforms = this.intersectionMaterialUniforms;
    this.sliceMaterial.uniforms = this.sliceMaterialUniforms;
  }

  /**
   * Append the canvas element that contains the webgl 
   * slicing context to the given div. 
   * 
   * Existing children of the div are removed!
   * 
   * @param div the div to append this element to. 
   */
  rehomeTo(div: HTMLDivElement) {
    div.innerHTML = "";
    div.appendChild(this.renderer.domElement);
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
    this.scene.printVolume.visible = false
    // Set model color to white,
    this.scene.overrideMaterial = core.CoreMaterialsFactory.flatWhiteMaterial;
    // Hide slice background if present, render to temp target 1;
    this.sliceBackground.visible = false;
    this.renderer.render(this.scene, this.sliceCamera, this.tempTarget1, true);

    // Hide objects, show slice background
    this.scene.hidePrintObjects();
    this.sliceBackground.visible = true;
    // Apply dialate filter to texture
    if (this.raftDilatePixels > 0) {
      this.scene.overrideMaterial = this.erodeDialateMaterial;
      this.erodeDialateMaterialUniforms.dilate.value = 1;
      let dilatePixels = this.raftDilatePixels;
      // Repeatedly apply dilate if needed
      while (dilatePixels > 0) {
        let pixels = dilatePixels % 10 || 10;
        dilatePixels = dilatePixels - pixels;
        this.erodeDialateMaterialUniforms.src = new core.TextureUniform(this.tempTarget1.texture);
        this.erodeDialateMaterialUniforms.pixels.value = pixels;
        this.erodeDialateMaterial.needsUpdate = true;
        this.renderer.render(this.scene, this.sliceCamera, this.tempTarget2, true);
        let swapTarget = this.tempTarget2;
        this.tempTarget2 = this.tempTarget1;
        this.tempTarget1 = swapTarget;
      }
    }
    // render texture to view
    this.renderSliceFinal(this.tempTarget2);
  }

  private renderSlice(z: number) {

    this.renderSliceCommon(z);
    if (this.shellErodePixels > 0) {
      this.renderShelledSlice2(z);
      this.renderSliceFinal(this.finalCompositeTarget);
    } else {
      this.renderSliceFinal(this.maskTarget);
    }
  }

  private renderShelledSlice() {
    this.erodeOrDilate(this.shellErodePixels, false);
    // X-Y shelling to temp target 1
    this.scene.overrideMaterial = this.xorMaterial;
    this.xorMaterialUniforms.src1 = new core.TextureUniform(this.maskTarget.texture);
    this.xorMaterialUniforms.src2 = new core.TextureUniform(this.tempTarget1.texture);
    this.xorMaterial.needsUpdate = true;
    this.renderer.render(this.scene, this.sliceCamera, this.finalCompositeTarget, true);
  }

  private renderShelledSlice2(z: number) {
    // Do Z direction shell step...
    // Look Down
    this.sliceMaterialUniforms.cutoff.value = 2.0;
    this.zShellCamera.position.z = this.scene.printVolume.boundingBox.max.z;
    this.zShellCamera.near = z;
    this.zShellCamera.far = z + this.shellInset;
    this.zShellCamera.lookAt(NEG_Z);
    this.zShellCamera.up = POS_Y;
    this.zShellCamera.updateProjectionMatrix();
    this.renderer.render(this.scene, this.zShellCamera, this.tempTarget1, true);
    // Look Up
    this.zShellCamera.position.z = 0;
    this.zShellCamera.near = z;
    this.zShellCamera.far = z + this.shellInset;
    this.zShellCamera.lookAt(POS_Z);
    this.zShellCamera.up = NEG_Y;
    this.zShellCamera.updateProjectionMatrix();
    this.renderer.render(this.scene, this.zShellCamera, this.tempTarget2, true);
    // Use OR material to combine
    this.sliceBackground.visible = true;
    this.scene.overrideMaterial = this.orMaterial;
    this.orMaterialUniforms.src1 = new core.TextureUniform(this.tempTarget1.texture);
    this.orMaterialUniforms.src2 = new core.TextureUniform(this.tempTarget2.texture);
    this.orMaterial.needsUpdate = true;
    this.renderer.render(this.scene, this.sliceCamera, this.zshellTarget, true);

    this.erodeOrDilate(this.shellErodePixels, false);
    // X-Y shelling to temp target 1
    this.scene.overrideMaterial = this.xorMaterial;
    this.xorMaterialUniforms.src1 = new core.TextureUniform(this.maskTarget.texture);
    this.xorMaterialUniforms.src2 = new core.TextureUniform(this.tempTarget1.texture);
    this.xorMaterial.needsUpdate = true;
    this.renderer.render(this.scene, this.sliceCamera, this.tempTarget1, true);
    // OR X-Y with Z shell
    this.scene.overrideMaterial = this.orMaterial;
    this.orMaterialUniforms.src1 = new core.TextureUniform(this.zshellTarget.texture);
    this.orMaterialUniforms.src2 = new core.TextureUniform(this.tempTarget1.texture);
    this.orMaterial.needsUpdate = true;
    this.renderer.render(this.scene, this.sliceCamera, this.finalCompositeTarget, true);
  }

  private renderCombinedSlice() {

  }

  /**
  * Render a slice of scene at z to maskTarget
  */
  private renderSliceCommon(z: number) {
    // Hide print volume
    this.scene.printVolume.visible = false
    // Hide slice background
    this.sliceBackground.visible = false;
    this.scene.showPrintObjects();
    let buildVolHeight = this.scene.printVolume.boundingBox.max.z;
    let sliceZ = (FAR_Z_PADDING + z) / (FAR_Z_PADDING + buildVolHeight);
    // Intersection test material to temp2
    this.scene.overrideMaterial = this.intersectionTestMaterial;
    this.intersectionMaterialUniforms.cutoff.value = sliceZ;
    this.intersectionTestMaterial.needsUpdate = true;
    this.renderer.render(this.scene, this.sliceCamera, this.tempTarget2, true);
    // Render slice to maskTarget
    this.scene.overrideMaterial = this.sliceMaterial;
    this.sliceMaterialUniforms.iTex = new core.TextureUniform(this.tempTarget2.texture);
    this.sliceMaterialUniforms.cutoff.value = sliceZ;
    this.sliceMaterial.needsUpdate = true;
    this.renderer.render(this.scene, this.sliceCamera, this.maskTarget, true);
    if (this.shellErodePixels > 0) {
      this.renderer.render(this.scene, this.sliceCamera, this.tempTarget1, true);
    }
  }

  /**
  * Erode or dilate the image in tempTarget1
  *
  * The final image is in tempTarget1
  *
  * uses tempTarget1 and tempTarget2
  */
  private erodeOrDilate(numPixels: number, dilate: boolean) {
    // Hide objects, show slice background
    this.scene.hidePrintObjects();
    this.sliceBackground.visible = true;
    // Apply dialate filter to texture
    if (numPixels > 0) {
      this.scene.overrideMaterial = this.erodeDialateMaterial;
      this.erodeDialateMaterialUniforms.dilate.value = dilate ? 1 : 0;
      let dilatePixels = numPixels;
      // Repeatedly apply dilate if needed
      while (dilatePixels > 0) {
        let pixels = dilatePixels % 10 || 10;
        dilatePixels = dilatePixels - pixels;
        this.erodeDialateMaterialUniforms.src = new core.TextureUniform(this.tempTarget1.texture);
        this.erodeDialateMaterialUniforms.pixels.value = pixels;
        this.erodeDialateMaterial.needsUpdate = true;
        this.renderer.render(this.scene, this.sliceCamera, this.tempTarget2, true);
        let swapTarget = this.tempTarget1;
        this.tempTarget1 = this.tempTarget2;
        this.tempTarget2 = swapTarget;
      }
    }
  }

  /**
  * Display the final slice image stored in srcTarget copying it to the display
  */
  private renderSliceFinal(srcTarget: THREE.WebGLRenderTarget) {
    // Render final image
    this.scene.overrideMaterial = this.copyMaterial;
    // Hide objects, show slice background
    this.scene.hidePrintObjects();
    this.sliceBackground.visible = true;
    // this.copyMaterialUniforms.src = new core.TextureUniform(this.finalCompositeTarget);
    this.copyMaterialUniforms.src = new core.TextureUniform(srcTarget.texture);
    this.copyMaterial.needsUpdate = true;
    this.renderer.render(this.scene, this.sliceCamera);
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
      this.sliceBackground.scale.x = width + 20;
      this.sliceBackground.scale.y = height + 20;
      this.lastWidth = width;
      this.lastHeight = height;
      dirty = true;
      // window.console.log(this);
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
    this.sliceBackground.visible = false;
    this.scene.printVolume.visible = true;
    this.scene.showPrintObjects();
  }

  /**
  * reallocate the rendering Targets
  */
  private reallocateTargets(width: number, height: number) {
    // Dispose
    this.finalCompositeTarget && this.finalCompositeTarget.dispose();
    this.maskTarget && this.maskTarget.dispose();
    this.tempTarget1 && this.tempTarget1.dispose();
    this.tempTarget2 && this.tempTarget2.dispose();
    this.zshellTarget && this.zshellTarget.dispose();
    // Allocate
    this.finalCompositeTarget = new THREE.WebGLRenderTarget(width, height, {
      format: THREE.RGBAFormat,
      depthBuffer: true,
      stencilBuffer: false,
      // generateMipMaps: false,
      minFilter: THREE.NearestFilter,
      magFilter: THREE.NearestFilter,
      wrapS: THREE.ClampToEdgeWrapping,
      wrapT: THREE.ClampToEdgeWrapping
    });
    this.maskTarget = new THREE.WebGLRenderTarget(width, height, {
      format: THREE.RGBAFormat,
      depthBuffer: true,
      stencilBuffer: false,
      // generateMipMaps: false,
      minFilter: THREE.NearestFilter,
      magFilter: THREE.NearestFilter,
      wrapS: THREE.ClampToEdgeWrapping,
      wrapT: THREE.ClampToEdgeWrapping
    });
    this.tempTarget1 = new THREE.WebGLRenderTarget(width, height, {
      format: THREE.RGBAFormat,
      depthBuffer: true,
      stencilBuffer: false,
      // generateMipMaps: false,
      minFilter: THREE.NearestFilter,
      magFilter: THREE.NearestFilter,
      wrapS: THREE.ClampToEdgeWrapping,
      wrapT: THREE.ClampToEdgeWrapping
    });
    this.tempTarget2 = new THREE.WebGLRenderTarget(width, height, {
      format: THREE.RGBAFormat,
      depthBuffer: true,
      stencilBuffer: false,
      // generateMipMaps: false,
      minFilter: THREE.NearestFilter,
      magFilter: THREE.NearestFilter,
      wrapS: THREE.ClampToEdgeWrapping,
      wrapT: THREE.ClampToEdgeWrapping
    });
    this.zshellTarget = new THREE.WebGLRenderTarget(width, height, {
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
}
