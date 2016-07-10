module microtome.slicer {

  // TODO Move to threed?
  const Z_DOWN = new THREE.Vector3(0, 0, -1000000);
  const Z_UP = new THREE.Vector3(0, 0, 1000000);
  const ORIGIN = new THREE.Vector3(0, 0, 0);
  const POS_X = new THREE.Vector3(1, 0, 0);
  const NEG_X = new THREE.Vector3(-1, 0, 0);
  const POS_Y = new THREE.Vector3(0, 1, 0);
  const NEG_Y = new THREE.Vector3(0, -1, 0);
  const POS_Z = new THREE.Vector3(0, 0, 1);
  const NEG_Z = new THREE.Vector3(0, 0, -1);

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

    // Materials ---------------------------------------------------------------------------------

    private erodeDialateMaterial = three_d.CoreMaterialsFactory.erodeOrDialateMaterial.clone();

    private erodeDialateMaterialUniforms = new three_d.ErodeDialateShaderUniforms(
      new three_d.IntegerUniform(1),
      new three_d.IntegerUniform(0),
      new three_d.TextureUniform(null),
      new three_d.IntegerUniform(0),
      new three_d.IntegerUniform(0));

    private copyMaterial = three_d.CoreMaterialsFactory.copyMaterial.clone();

    private copyMaterialUniforms = new three_d.CopyShaderUniforms(
      new three_d.TextureUniform(null),
      new three_d.IntegerUniform(0),
      new three_d.IntegerUniform(0));

    private xorMaterial = three_d.CoreMaterialsFactory.xorMaterial.clone();
    private xorMaterialUniforms = new three_d.BoolOpShaderUniforms(
      new three_d.TextureUniform(null),
      new three_d.TextureUniform(null),
      new three_d.IntegerUniform(0),
      new three_d.IntegerUniform(0));

    private orMaterial = three_d.CoreMaterialsFactory.orMaterial.clone();
    private orMaterialUniforms = new three_d.BoolOpShaderUniforms(
      new three_d.TextureUniform(null),
      new three_d.TextureUniform(null),
      new three_d.IntegerUniform(0),
      new three_d.IntegerUniform(0));

    private intersectionTestMaterial = three_d.CoreMaterialsFactory.intersectionMaterial.clone();

    private intersectionMaterialUniforms = new three_d.IntersectionShaderUniforms(
      new three_d.FloatUniform(0)
    );

    private sliceMaterial = three_d.CoreMaterialsFactory.sliceMaterial.clone();

    private sliceMaterialUniforms = new three_d.SliceShaderUniforms(
      new three_d.FloatUniform(0),
      new three_d.TextureUniform(null),
      new three_d.IntegerUniform(0),
      new three_d.IntegerUniform(0));

    /**
    *
    */
    constructor(
      private scene: microtome.three_d.PrinterScene,
      public pixelWidthMM: number,
      public pixelHeightMM: number,
      public raftThickness: number,
      public raftOffset: number,
      public shellInset: number,
      private renderer?: THREE.WebGLRenderer) {
      var planeGeom: THREE.PlaneGeometry = new THREE.PlaneGeometry(1.0, 1.0);
      var planeMaterial = microtome.three_d.CoreMaterialsFactory.whiteMaterial.clone();
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
      this.render(z);
      return this.renderer.domElement.toDataURL();
    }

    private render(z: number) {
      try {
        let dirty = this.prepareRender();
        if (z <= this.raftThickness) {
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
      this.scene.overrideMaterial = microtome.three_d.CoreMaterialsFactory.flatWhiteMaterial;
      // Hide slice background if present, render to temp target 1;
      this.sliceBackground.visible = false;
      this.renderer.render(this.scene, this.sliceCamera, this.tempTarget2, true);

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
          this.erodeDialateMaterialUniforms.src = new three_d.TextureUniform(this.tempTarget1);
          this.erodeDialateMaterialUniforms.pixels.value = pixels;
          this.erodeDialateMaterial.needsUpdate = true;
          this.renderer.render(this.scene, this.sliceCamera, this.tempTarget1, true);
          let swapTarget = this.tempTarget2;
          this.tempTarget2 = this.tempTarget1;
          this.tempTarget1 = swapTarget;
        }
      }
      // render texture to view
      this.renderSliceFinal(this.tempTarget1);
    }

    private renderSlice(z: number) {
      this.renderSliceCommon(z);
      if (this.shellErodePixels > 0) {
        this.renderShelledSlice();
        this.renderSliceFinal(this.finalCompositeTarget);
      } else {
        this.renderSliceFinal(this.maskTarget);
      }
    }

    private renderShelledSlice() {
      /*
              // Do Z direction shell step...
              // Look Down
              this.sliceMaterialUniforms.cutoff.value = 2.0;
              this.zShellCamera.position.z = this.scene.printVolume.boundingBox.max.z;
              this.zShellCamera.near = z;
              this.zShellCamera.far = z + this.shellInset;
              this.zShellCamera.lookAt(NEG_Z);
              this.zShellCamera.up = POS_Y;
              this.zShellCamera.updateProjectionMatrix();
              this.renderer.render(this.scene, this.zShellCamera, this.tempTarget2, true);
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
              this.orMaterialUniforms.src1 = new three_d.TextureUniform(this.tempTarget1);
              this.orMaterialUniforms.src2 = new three_d.TextureUniform(this.tempTarget2);
              this.orMaterial.needsUpdate = true;
              this.renderer.render(this.scene, this.sliceCamera, this.zshellTarget, true);
      */
      this.erodeOrDilate(this.shellErodePixels, false);
      // X-Y shelling to temp target 1
      this.scene.overrideMaterial = this.xorMaterial;
      this.xorMaterialUniforms.src1 = new three_d.TextureUniform(this.maskTarget);
      this.xorMaterialUniforms.src2 = new three_d.TextureUniform(this.tempTarget1);
      this.xorMaterial.needsUpdate = true;
      // this.renderer.render(this.scene, this.sliceCamera, this.tempTarget1, true);
      this.renderer.render(this.scene, this.sliceCamera, this.finalCompositeTarget, true);
      // OR X-Y with Z shell
      // this.scene.overrideMaterial = this.orMaterial;
      // this.orMaterialUniforms.src1 = new three_d.TextureUniform(this.zshellTarget);
      // this.orMaterialUniforms.src2 = new three_d.TextureUniform(this.tempTarget1);
      // this.orMaterial.needsUpdate = true;
      // this.renderer.render(this.scene, this.sliceCamera, this.finalCompositeTarget, true);
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
      // window.console.log(`Render Slice At m:${z} => ${z / buildVolHeight} => ${sliceZ}`);
      // Intersection test material to temp2
      this.scene.overrideMaterial = this.intersectionTestMaterial;
      this.intersectionMaterialUniforms.cutoff.value = sliceZ;
      this.intersectionTestMaterial.needsUpdate = true;
      this.renderer.render(this.scene, this.sliceCamera, this.tempTarget2, true);
      // this.renderer.render(this.scene, this.sliceCamera, this.finalCompositeTarget, true);
      // Render slice to maskTarget
      this.scene.overrideMaterial = this.sliceMaterial;
      this.sliceMaterialUniforms.iTex = new three_d.TextureUniform(this.tempTarget2);
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
          this.erodeDialateMaterialUniforms.src = new three_d.TextureUniform(this.tempTarget1);
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
      // this.copyMaterialUniforms.src = new three_d.TextureUniform(this.finalCompositeTarget);
      this.copyMaterialUniforms.src = new three_d.TextureUniform(srcTarget);
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
        window.console.log(this);
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
}
