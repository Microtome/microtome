import * as microtome from "microtome";

type PrinterScene = microtome.printer.PrinterScene;

/**
 * The slice preview previews slices
 */
export class SlicePreview {

  public disabled: boolean = false;

  public sliceAt: number;

  private _reqAnimFrameHandle: number;

  private _slicer: microtome.slicer.AdvancedSlicer;

  constructor(private _canvasHome: HTMLDivElement, private scene: PrinterScene) {
    this.attached();
    this._slicer = new microtome.slicer.AdvancedSlicer(
      this.scene, 0.1, 0.1, 1.5, 5, 0, _canvasHome);
  }

  public attached() {
    this._startRendering();
  }

  public detached() {
    this._stopRendering();
  }

  public disabledChanged(newValue: boolean) {
    if (!newValue) {
      this._startRendering();
    } else {
      this._stopRendering();
    }
  }

  private _stopRendering() {
    if (this._slicer) {
      if (this._reqAnimFrameHandle) {
        window.cancelAnimationFrame(this._reqAnimFrameHandle);
      }
      this.scene.printVolume.visible = true;
    }
  }

  private _startRendering() {
    if (this._slicer) {
      if (this._reqAnimFrameHandle) {
        window.cancelAnimationFrame(this._reqAnimFrameHandle);
      }
    }
    this.scene.printVolume.visible = false;
    this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
  }

  private _render() {
    if (this.disabled) {
      this._stopRendering();
      return;
    }
    const div = this._canvasHome;
    const pvw = this.scene.printVolume.width;
    const pvd = this.scene.printVolume.depth;
    const scaleh = div.clientHeight / pvd;
    const scalew = div.clientWidth / pvw;

    const scale = scaleh < scalew ? scaleh : scalew;
    this._slicer.setSize(pvw * scale, pvd * scale);
    // TODO fix NEED dirty check on div resize
    this._slicer.sliceAt(this.sliceAt);
    this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
  }
}
