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
    depthTarget:THREE.WebGLRenderTarget = null;

    // Temp target 1
    tempTarget1: THREE.WebGLRenderTarget = null;

    // Temp target 2
    tempTarget2: THREE.WebGLRenderTarget = null;

    /**
    *
    */
    constructor(
      private scene: microtome.three_d.PrinterScene,
      private raftThickness: number,
      private width: number,
      private height: number,
      private renderer?: THREE.WebGLRenderer,
      maxZ?: number) {
      this.initialize(width, height, maxZ)
    }

    /**
    * Scene has changed, so certain things need to be reset
    */
    markSceneDirty() {

    }

    /**
    Initialize the advanced slicer to the given settings, allowing it to be reused
    */
    initialize(raftThickness?: number, width?: number, height?: number, maxZ?: number) {
      // Initialize the advanced slicer to the given settings, allowing it to be reused
    }

    /**
    * Slice the scene at the given z offset in mm.
    */
    sliceAt(z: number) {
    }

    sliceAtToImage(z:number):String {
      this.sliceAt(z);
      return this.renderer.domElement.toDataURL();
    }

    _renderRaftSlice(){
      // Set model color to white,
      // Hide slice background
      // render to texture
      // Apply dialate filter
      // Show slice background
      // render.
    }

    _renderSlice(z:number){
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

    /**
    * Allocate the rendering Targets
    */
    private _allocateTargets(){

    }

    /**
    * Dispose of all rendering targets
    */
    private _disposeTargets(){

    }
  }
}
