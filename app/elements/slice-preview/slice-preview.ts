
@component("slice-preview")
class SlicePreview extends polymer.Base {

  private _renderer: THREE.WebGLRenderer = new THREE.WebGLRenderer({ alpha: false, antialias: false, clearColor: 0x000000 });

  private _canvasElement: HTMLCanvasElement = this._renderer.domElement;

  private static _ORIGIN = new THREE.Vector3(0, 0, 0);

  private _pvObjectGroup = new THREE.Group();

  private _canvasHome: HTMLDivElement;

  private _reqAnimFrameHandle: number;

  private _slicer: microtome.slicer.Slicer = new microtome.slicer.Slicer(this.scene, this._renderer);

  @property({ notify: false, readOnly: false })
  public scene: microtome.three_d.PrinterScene;

  @property({ notify: true, readOnly: false, reflectToAttribute: true, type: Boolean })
  public disabled: boolean = false;

  @property({ notify: false, readOnly: false, type: Number })
  public xres: number;

  @property({ notify: false, readOnly: false, type: Number })
  public yres: number;

  @property({ notify: false, readOnly: false, type: Number })
  public sliceAt: number;

  public attached() {
    // this._canvasElement.className += " fit"
    this._canvasHome = this.$["slice-canvas-home"] as HTMLDivElement;
    this._canvasHome.appendChild(this._canvasElement);
    this._startRendering();
  }

  public detached() {
    this._slicer.teardownSlicerPreview();
    this._stopRendering();
  }

  @observe("disabled")
  disabledChanged(newValue: boolean, oldValue: boolean) {
    if (!newValue) {
      this._startRendering();
    } else {
      this._stopRendering();
    }
  }


  private _stopRendering() {
    if (this._reqAnimFrameHandle) window.cancelAnimationFrame(this._reqAnimFrameHandle)
    this._slicer.teardownSlicerPreview();
    this.scene.printVolume.visible = true;
  }

  private _startRendering() {
    if (this._reqAnimFrameHandle) window.cancelAnimationFrame(this._reqAnimFrameHandle);
    this._slicer.setupSlicerPreview();
    this.scene.printVolume.visible = false;
    this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
  }


  private _render(timestamp: number) {
    if (this.disabled) {
      this._stopRendering();
      return;
    }
    var canvas = this._canvasElement;
    var div = this._canvasHome
    var pvw = this.scene.printVolume.width;
    var pvd = this.scene.printVolume.depth;
    var scaleh = div.clientHeight / pvd;
    var scalew = div.clientWidth / pvw;
    var scale = scaleh < scalew ? scaleh : scalew;
    this._renderer.setSize(this.xres, this.yres);
    canvas.style.width = "" + pvw * scale + "px";
    canvas.style.height = "" + pvd * scale + "px";
    // TODO fix NEED dirty check on div resize
    this._slicer.resize();
    this._slicer.sliceAt(this.sliceAt);
    this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
  }
}

SlicePreview.register();
