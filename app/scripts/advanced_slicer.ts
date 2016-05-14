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
    private _sliceCamera: THREE.OrthographicCamera = null;
    // z-shell camera
    private _zShellCamera: THREE.OrthographicCamera = null;

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
    private _lastWidth: number = -1;
    // Cache canvas height for dirty checking
    private _lastHeight: number = -1;

    // Width of a pixel in mm
    private _pixelWidthMM = -1;
    // Height of a pixel in mm
    private _pixelHeightMM = -1;
    // Shell pixels in x dir
    private _shellPixelsX = -1;
    // Shell pixels in y dir
    private _shellPixelsY = -1;
    // Raft outset pixels in x dir
    private _raftOutsetPixelsX = -1;
    // Raft outset pixels in y dir
    private _raftOutsetPixelsY = -1;

    // In order for a shell to be fully connected, min pixel
    // count is 3 in x or y.
    private static _MIN_SHELL_PIXELS = 3;

    //Structuring element sampling offsets for shelling
    private _shellSElemSampOffsets: number[] = [];
    //Structuring element sampling offsets for raft outset
    private _raftSElemSampOffsets: number[] = [];

    private _sliceBackground: THREE.Mesh;

    private _erodeDialateMaterialUniforms = {
      'dialate': { type: 'i', value: 0 },
      'pixelRadius': { type: 'f', value: AdvancedSlicer._MIN_SHELL_PIXELS },
      'src': { type: 't', value: <THREE.WebGLRenderTarget>null },
      'viewWidth': { type: 'i', value: 0.0 },
      'viewHeight': { type: 'i', value: 0.0 }
    };

    private _copyMaterialUniforms = {
      'src': { type: 't', value: <THREE.WebGLRenderTarget>null },
      'viewWidth': { type: 'i', value: 0.0 },
      'viewHeight': { type: 'i', value: 0.0 }
    };

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
      this._sliceBackground = new THREE.Mesh(planeGeom, planeMaterial);
      this._sliceBackground.position.z = SLICER_BACKGROUND_Z;
      this._sliceCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5);
      this._zShellCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5);
    }

    /**
    * Slice the scene at the given z offset in mm.
    */
    sliceAt(z: number) {
      this._render(z);
    }

    /***
    * Slice to an image
    * Returns a dataurl of the image
    */
    sliceAtToImage(z: number): String {
      this.sliceAt(z);
      return this.renderer.domElement.toDataURL();
    }

    private _render(z: number) {
      let dirty = this._prepareRender();
      if (z <= this.raftThickness) {
        this._renderRaftSlice();
      } else {
        this._renderSlice(z, dirty);
      }
    }

    private _renderRaftSlice() {

      // Set model color to white,
      this.scene.overrideMaterial = microtome.three_d.CoreMaterialsFactory.whiteMaterial
      // Hide slice background if present
      this.scene.remove(this._sliceBackground);
      // render to texture
      this.renderer.render(this.scene, this._sliceCamera, this.tempTarget1, true)
      // Apply dialate filter to texture

      // Show slice background
      this.scene.add(this._sliceBackground);
      // render texture to view
    }

    /**
    * Handle reallocating render targets if dimensions have changed and
    * return true if changes have occured
    */
    private _prepareRender(): boolean {
      let dirty = false;
      // Get canvasElement height
      let width = this.renderer.domElement.width;
      let height = this.renderer.domElement.height;
      // If its changed...
      if (width != this._lastWidth || height != this._lastHeight) {
        // Recalc pixel width, and shelling parameters
        this._pixelWidthMM = this.scene.printVolume.width / width;
        this._pixelHeightMM = this.scene.printVolume.depth / height;
        // TODO NOT needed for now, will need to reapproach for non square pixel densities
        // Also currently all done in the shader.
        // this._recalcShellSElemSampOffsets();
        // this._recalcRaftSElemSampOffsets();
        // Dispose old textures
        // Allocate new textures
        this._reallocateTargets(width, height)
        this._lastWidth = width;
        this._lastHeight = height;
        dirty = true;
      }
      return dirty;
    }

    /**
    * Update camera dimensions
    */
    private _prepareCameras(newWidth: number, newHeight: number) {
      var pVolumeBBox = this.scene.printVolume.boundingBox;

      var widthRatio: number = Math.abs(pVolumeBBox.max.x - pVolumeBBox.min.x) / newWidth;
      var heightRatio: number = Math.abs(pVolumeBBox.max.y - pVolumeBBox.min.y) / newHeight;
      var scale: number = widthRatio > heightRatio ? widthRatio : heightRatio;
      let right = (scale * newWidth) / 2.0;
      let left = -right;
      let top = (scale * newHeight) / 2.0;
      let bottom = -top
      for (let camera of [this._sliceCamera, this._zShellCamera]) {
        camera.right = right;
        camera.left = left;
        camera.top = top;
        camera.bottom = bottom;
        camera.updateProjectionMatrix();
        camera.lookAt(Z_DOWN);
      }
    }

    /**
    * Update shader uniforms such as dimensions
    */
    private _prepareShaders(newWidth: number, newHeight: number) {

    }

    private _renderSlice(z: number, dirty: boolean) {

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

    // private _recalcShellSElemSampOffsets() {
    //   this._shellPixelsX = this.shellThickness / this._pixelWidthMM;
    //   this._shellPixelsY = this.shellThickness / this._pixelHeightMM;
    //   if (this._shellPixelsX < AdvancedSlicer._MIN_SHELL_PIXELS) {
    //     window.console.warn(`Too few x shell pixels: ${this._shellPixelsX}`);
    //     this._shellPixelsX = AdvancedSlicer._MIN_SHELL_PIXELS;
    //   }
    //   if (
    //     this._shellPixelsY < AdvancedSlicer._MIN_SHELL_PIXELS) {
    //     window.console.warn(`Too few y shell pixels: ${this._shellPixelsY}`);
    //     this._shellPixelsX = AdvancedSlicer._MIN_SHELL_PIXELS;
    //   }
    //
    // }
    //
    // private _recalcRaftSElemSampOffsets() {
    //
    // }

    /**
    * reallocate the rendering Targets
    */
    private _reallocateTargets(width: number, height: number) {
      // Dispose
      this.finalCompositeTarget.dispose();
      this.depthTarget.dispose();
      this.maskTarget.dispose();
      this.tempTarget1.dispose();
      this.tempTarget2.dispose();
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

    // private _preSlice() {
    //   this.scene.add(this._sliceBackground);
    //   this.targetZ = this.scene.printVolume.height;
    // }
    //
    // private _postSlice() {
    //   this.scene.overrideMaterial = null;
    //   this.scene.remove(this._sliceBackground);
    // }

    /**
    * Dispose of all rendering targets
    */
  }
}
