module microtome.slicer {

  /**
  Advanced slicer supporting shelling, support pattern generation,
  hollow shells, etc.
  */
  export class AdvancedSlicer {

    // Final composition target where final image is composed
    finalCompositeTarget: THREE.WebGLRenderTarget = null;

    // Because of how we slice, we can't use stencil buffer
    maskTarget: THREE.WebGLRenderTarget = null;

    // Depth target
    depthTarget: THREE.WebGLRenderTarget = null;

    // Temp target 1
    tempTarget1: THREE.WebGLRenderTarget = null;

    // Temp target 2
    tempTarget2: THREE.WebGLRenderTarget = null;

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
      'dialate': { type: 'i', value: 0 },
      'pixelRadius': { type: 'f', value: AdvancedSlicer._MIN_SHELL_PIXELS },
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
    }

    /**
    * Slice the scene at the given z offset in mm.
    */
    sliceAt(z: number) {
      this._render(z);
    }

    sliceAtToImage(z: number): String {
      this.sliceAt(z);
      return this.renderer.domElement.toDataURL();
    }

    private _render(z: number) {
      if (z <= this.raftThickness) {
        this._renderRaftSlice();
      } else {
        this._renderSlice(z);
      }
    }

    private _renderRaftSlice() {

      // Set model color to white,
      this.scene.overrideMaterial = microtome.three_d.CoreMaterialsFactory.whiteMaterial
      // Hide slice background if present
      this.scene.remove(this._sliceBackground);
      // render to texture
      // Apply dialate filter
      // Show slice background
      this.scene.add(this._sliceBackground);
      // render.
    }

    _renderSlice(z: number) {

      // Get canvasElement height
      var width = this.renderer.domElement.width;
      var height = this.renderer.domElement.height;
      // If its changed...
      if (width != this._lastWidth || height != this._lastHeight) {
        // Recalc pixel width, and shelling parameters
        this._pixelWidthMM = this.scene.printVolume.width / width;
        this._pixelHeightMM = this.scene.printVolume.depth / height;
        this._recalcShellSElemSampOffsets();
        this._recalcRaftSElemSampOffsets();
        // Dispose old textures
        this._disposeTargets();
        // Allocate new textures
        this._allocateTargets(width, height);
        // Generate depth image


        this._lastWidth = width;
        this._lastHeight = height;
      }
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

    private _recalcShellSElemSampOffsets() {
      this._shellPixelsX = this.shellThickness / this._pixelWidthMM;
      this._shellPixelsY = this.shellThickness / this._pixelHeightMM;
      if (this._shellPixelsX < AdvancedSlicer._MIN_SHELL_PIXELS) {
        window.console.warn(`Too few x shell pixels: ${this._shellPixelsX}`);
        this._shellPixelsX = AdvancedSlicer._MIN_SHELL_PIXELS;
      }
      if (
        this._shellPixelsY < AdvancedSlicer._MIN_SHELL_PIXELS) {
        window.console.warn(`Too few y shell pixels: ${this._shellPixelsY}`);
        this._shellPixelsX = AdvancedSlicer._MIN_SHELL_PIXELS;
      }

    }

    private _recalcRaftSElemSampOffsets() {

    }

    /**
    * Allocate the rendering Targets
    */
    private _allocateTargets(width: number, height: number) {
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
    private _disposeTargets() {
      this.finalCompositeTarget.dispose();
      this.maskTarget.dispose();
      this.tempTarget1.dispose();
      this.tempTarget2.dispose();
    }
  }
}
