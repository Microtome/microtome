module microtome.slicer {

  // TODO Move to threed?
  const Z_DOWN = new THREE.Vector3(0, 0, -1000000);
  const Z_UP = new THREE.Vector3(0, 0, 1000000);

  /**
  Advanced slicer supporting shelling, support pattern generation,
  hollow shells, etc.
  */
  export class AdvancedSlicer {
    // Slice camera
    private sliceCamera: THREE.OrthographicCamera = null;
    // z-shell camera
    private zShellCamera: THREE.OrthographicCamera = null;

    // Final composition target where final image is composed
    private finalCompositeTarget: THREE.WebGLRenderTarget = null;

    // Because of how we slice, we can't use stencil buffer
    private maskTarget: THREE.WebGLRenderTarget = null;

    // Depth target
    private depthTarget: THREE.WebGLRenderTarget = null;

    // Temp target 1
    private tempTarget1: THREE.WebGLRenderTarget = null;

    // Temp target 2
    private tempTarget2: THREE.WebGLRenderTarget = null;

    // Cache canvas width for dirty checking
    private lastWidth: number = -1;
    // Cache canvas height for dirty checking
    private lastHeight: number = -1;

    // Width of a pixel in mm
    private pixelWidthMM = -1;
    // Height of a pixel in mm
    private pixelHeightMM = -1;
    // Shell pixels in x dir
    private shellPixelsX = -1;
    // Shell pixels in y dir
    private shellPixelsY = -1;
    // Raft outset pixels in x dir
    private raftOutsetPixelsX = -1;
    // Raft outset pixels in y dir
    private raftOutsetPixelsY = -1;

    // In order for a shell to be fully connected, min pixel
    // count is 3 in x or y.
    private static _MIN_SHELL_PIXELS = 3;

    // //Structuring element sampling offsets for shelling
    // private shellSElemSampOffsets: number[] = [];
    // //Structuring element sampling offsets for raft outset
    // private raftSElemSampOffsets: number[] = [];

    private sliceBackground: THREE.Mesh;

    // Materials ---------------------------------------------------------------------------------

    private erodeDialateMaterial = three_d.CoreMaterialsFactory.erodeOrDialateMaterial.clone();

    private erodeDialateMaterialUniforms = new three_d.ErodeDialateShaderUniforms(
      new three_d.IntegerUniform(1),
      new three_d.IntegerUniform(0),
      new three_d.TextureUniform(null),
      new three_d.IntegerUniform(0),
      new three_d.IntegerUniform(0)
    );

    private copyMaterial = three_d.CoreMaterialsFactory.copyMaterial.clone();

    private copyMaterialUniforms = new three_d.CopyShaderUniforms(
      new three_d.TextureUniform(null),
      new three_d.IntegerUniform(0),
      new three_d.IntegerUniform(0));

    /**
    *
    */
    constructor(
      private scene: microtome.three_d.PrinterScene,
      private raftThickness: number,
      private raftOffset: number,
      private shellThickness: number,
      private renderer?: THREE.WebGLRenderer,
      private maxZ?: number) {
      var planeGeom: THREE.PlaneGeometry = new THREE.PlaneGeometry(1.0, 1.0);
      var planeMaterial = microtome.three_d.CoreMaterialsFactory.whiteMaterial.clone();
      planeMaterial.side = THREE.DoubleSide;
      this.sliceBackground = new THREE.Mesh(planeGeom, planeMaterial);
      this.sliceBackground.position.z = SLICER_BACKGROUND_Z;
      this.sliceCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5);
      this.zShellCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5);

      this.erodeDialateMaterial.uniforms = this.erodeDialateMaterialUniforms;
      this.copyMaterial.uniforms = this.copyMaterialUniforms;
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
    sliceAtToImage(z: number): String {
      return this.renderer.domElement.toDataURL();
    }

    private render(z: number) {
      try {
        let dirty = this.prepareRender();
        // if (z <= this.raftThickness) {
        this.renderRaftSlice();
        // } else {
        // this.renderSlice(z, dirty);
        // }
      } finally {
        // Set everything back to normal if stuff goes south
        this.resetScene();
      }
    }

    private renderRaftSlice() {

      // Set model color to white,
      this.scene.overrideMaterial = microtome.three_d.CoreMaterialsFactory.flatWhiteMaterial;
      // Hide slice background if present
      this.scene.remove(this.sliceBackground);
      this.scene.printVolume.visible = false
      // this.renderer.render(this.scene, this.sliceCamera);
      // render to texture
      this.renderer.render(this.scene, this.sliceCamera, this.tempTarget1, true);
      // Hide objects, show slice background
      this.scene.hidePrintObjects();
      this.scene.add(this.sliceBackground);
      // Apply dialate filter to texture
      this.scene.overrideMaterial = this.erodeDialateMaterial;
      this.erodeDialateMaterialUniforms.src = new three_d.TextureUniform(this.tempTarget1);
      this.renderer.render(this.scene, this.sliceCamera, this.tempTarget2, true);
      // render texture to view
      this.scene.overrideMaterial = this.copyMaterial;
      this.copyMaterialUniforms.src = new three_d.TextureUniform(this.tempTarget2);
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
        // Recalc pixel width, and shelling parameters
        this.pixelWidthMM = this.scene.printVolume.width / width;
        this.pixelHeightMM = this.scene.printVolume.depth / height;
        // TODO NOT needed for now, will need to reapproach for non square pixel densities
        // Also currently all done in the shader.
        // this.recalcShellSElemSampOffsets();
        // this.recalcRaftSElemSampOffsets();
        // Dispose old textures
        // Allocate new textures
        this.prepareShaders(width, height);
        this.prepareCameras(width, height);
        this.reallocateTargets(width, height);
        this.lastWidth = width;
        this.lastHeight = height;
        this.sliceBackground.scale.x=width;
        this.sliceBackground.scale.y=height;
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
      for (let camera of [this.sliceCamera, this.zShellCamera]) {
        camera.right = right;
        camera.left = left;
        camera.top = top;
        camera.bottom = bottom;
        camera.updateProjectionMatrix();
        camera.lookAt(Z_DOWN);
      }
      let targetZ = this.scene.printVolume.boundingBox.max.z;
      this.sliceCamera.position.z = targetZ + CAMERA_NEAR;
      this.sliceCamera.near = CAMERA_NEAR;
      // We add a little padding to the camera far so that if
      // slice geometry is right on the 0 xy plane, when
      // we draw in the colors and textures we don't get ambiguity
      this.sliceCamera.far = FAR_Z_PADDING + targetZ + CAMERA_NEAR;

    }

    /**
    * Update shader uniforms such as dimensions
    */
    private prepareShaders(newWidth: number, newHeight: number) {
      this.erodeDialateMaterialUniforms.viewWidth.value = newWidth;
      this.erodeDialateMaterialUniforms.viewHeight.value = newHeight;
      this.copyMaterialUniforms.viewWidth.value = newWidth;
      this.copyMaterialUniforms.viewHeight.value = newHeight;
    }

    private renderSlice(z: number, dirty: boolean) {

      // Generate depth image

      // Generate slice mask image

      // Generate slice image with support

      // Copy slice mask

      // Erode slice Mask

      // Subtract eroded slice mask from slice w / support, using masking

      // Generate Z-shell help target

      // 1 Render top view of slice

      // 2 Render bottom view of slice

      // 3 Write combined texture, masked to final image
    }

    // private recalcShellSElemSampOffsets() {
    //   this.shellPixelsX = this.shellThickness / this.pixelWidthMM;
    //   this.shellPixelsY = this.shellThickness / this.pixelHeightMM;
    //   if (this.shellPixelsX < AdvancedSlicer.MIN_SHELL_PIXELS) {
    //     window.console.warn(`Too few x shell pixels: ${this.shellPixelsX}`);
    //     this.shellPixelsX = AdvancedSlicer.MIN_SHELL_PIXELS;
    //   }
    //   if (
    //     this.shellPixelsY < AdvancedSlicer.MIN_SHELL_PIXELS) {
    //     window.console.warn(`Too few y shell pixels: ${this.shellPixelsY}`);
    //     this.shellPixelsX = AdvancedSlicer.MIN_SHELL_PIXELS;
    //   }
    //
    // }
    //
    // private recalcRaftSElemSampOffsets() {
    //
    // }

    private resetScene() {
      this.scene.overrideMaterial = null;
      this.scene.remove(this.sliceBackground);
      this.scene.printVolume.visible = true;
      this.scene.showPrintObjects();
    }

    /**
    * reallocate the rendering Targets
    */
    private reallocateTargets(width: number, height: number) {
      // Dispose
      this.finalCompositeTarget && this.finalCompositeTarget.dispose();
      this.depthTarget && this.depthTarget.dispose();
      this.maskTarget && this.maskTarget.dispose();
      this.tempTarget1 && this.tempTarget1.dispose();
      this.tempTarget1 && this.tempTarget2.dispose();
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
      this.depthTarget = new THREE.WebGLRenderTarget(width, height, {
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
    }
  }
}
