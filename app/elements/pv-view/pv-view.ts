

@component("pv-view")
class PrinterVolumeView extends polymer.Base {

  private _renderer: THREE.Renderer = new THREE.WebGLRenderer({ alpha: true, antialias: true, clearColor: 0x000000, clearAlpha: 0 });

  private _scatterLight: THREE.AmbientLight = new THREE.AmbientLight();

  private _skyLight: THREE.DirectionalLight = new THREE.DirectionalLight();

  private _groundLight: THREE.DirectionalLight = new THREE.DirectionalLight();

  private _canvasElement: HTMLCanvasElement = this._renderer.domElement;

  private _pvCamera: THREE.PerspectiveCamera = new THREE.PerspectiveCamera(37, 1.0, 1.0, 2000.0);

  private static _ORIGIN = new THREE.Vector3(0, 0, 0);

  private _pvObjectGroup = new THREE.Group();

  private _canvasHome: HTMLDivElement;

  private _reqAnimFrameHandle: number;

  private _printerVolume: microtome.three_d.PrintVolume = new microtome.three_d.PrintVolume(120, 120, 120);

  private _camNav: microtome.three_d.CameraNav;

  @property({})
  public scene: microtome.three_d.PrinterScene;

  @property({ notify: true, readOnly: false })
  public disabled: boolean = false;

  @property({ notify: true, readOnly: false })
  public scatterColor: string = "#777777"

  @property({ notify: true, readOnly: false })
  public skyColor: string = "#AACCFF"

  @property({ notify: true, readOnly: false })
  public groundColor: string = "#775533"

  public attached() {
    // this._canvasElement.className += " fit"
    this._canvasHome = this.$["pv-canvas-home"] as HTMLDivElement;
    this._canvasHome.appendChild(this._canvasElement);
    // window.addEventListener("resize", (event) => this._resizeCanvas());
    this._pvCamera.up.set(0, 0, 1);
    this._pvCamera.position.set(0, 350, 250);
    this._configureLighting();
    // this.scene.add(this._pvCamera);
    this._pvCamera.lookAt(this._printerVolume.position);
    this._camNav = new microtome.three_d.CameraNav(this._pvCamera, this._canvasElement, true)
    this._startRendering();
  }

  public detached() {
    this._stopRendering();
  }

  private _configureLighting() {
    this._scatterLight.color.setStyle(this.scatterColor);
    this.scene.add(this._scatterLight);
    this._skyLight.color.setStyle(this.skyColor);
    this._skyLight.intensity = 0.55;
    this._skyLight.position.set(0, 0, 1000);
    this.scene.add(this._skyLight);
    this._groundLight.color.setStyle(this.groundColor);
    this._groundLight.intensity = 0.15;
    this._groundLight.position.set(0, 0, -1000);
    this.scene.add(this._groundLight);
  }

  // private _resizeCanvas() {
  //   var canvas = this._canvasElement;
  //   if (this._canvasHome) {
  //     console.log("RESIZE!")
  //     var div = this._canvasHome;
  //     canvas.width = div.clientWidth;
  //     canvas.height = div.clientHeight;
  //     // canvas.style.width = "${canvas.width}";
  //     // canvas.style.height = "${canvas.height}";
  //     this._pvCamera.aspect = canvas.clientWidth / canvas.clientHeight;
  //     window.console.log(this._pvCamera.aspect);
  //     this._pvCamera.updateProjectionMatrix();
  //   }
  // }

  @observe("disabled")
  disabledChanged(newValue: boolean, oldValue: boolean) {
    if (!newValue) {
      // this._camNav.enabled = false;
      this._startRendering();
    } else {
      // this._camNav.enabled = true;
      this._stopRendering();
    }
    if (this._camNav) {
      this._camNav.enabled = !newValue;
    }
  }

  private _stopRendering() {
    if (this._reqAnimFrameHandle) window.cancelAnimationFrame(this._reqAnimFrameHandle)
  }

  private _startRendering() {
    if (this._reqAnimFrameHandle) window.cancelAnimationFrame(this._reqAnimFrameHandle);
    this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
  }

  private _render(timestamp: number) {
    // TODO Race condition in start/stop rendering setting/unsetting material between
    // This guy and other scene.

    // TODO Parent app should handle "context switch" of this and slice preview
    // TODO And remove disabled flag from here and slice-preview.ts
    this.scene.overrideMaterial = null;

    var canvas = this._canvasElement;
    var div = this._canvasHome
    // var bounds = this._canvasHome.getBoundingClientRect()
    if (canvas.height != div.clientHeight || canvas.width != div.clientWidth) {
      canvas.width = div.clientWidth;
      canvas.height = div.clientHeight;
      this._pvCamera.aspect = div.clientWidth / div.clientHeight;
      this._pvCamera.updateProjectionMatrix();
      this._renderer.setSize(canvas.width, canvas.height);
    }
    this._renderer.render(this.scene, this._pvCamera);
    this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
  }

  @observe("scatterColor")
  scatterColorChanged(newValue: string, oldValue: string) {
    this._scatterLight.color.setStyle(newValue);
  }

  @observe("skyColor")
  skyColorChanged(newValue: string, oldValue: string) {
    this._skyLight.color.setStyle(newValue);
  }

  @observe("groundColor")
  groundColorChanged(newValue: string, oldValue: string) {
    this._groundLight.color.setStyle(newValue);
  }
}

PrinterVolumeView.register();
