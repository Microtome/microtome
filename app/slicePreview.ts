import * as microtome from "microtome"
import * as THREE from "three"

type PrinterScene = microtome.three_d.PrinterScene;

/**
 * The slice preview previews slices
 */
export class SlicePreview {

    private _renderer: THREE.WebGLRenderer = new THREE.WebGLRenderer({ alpha: false, antialias: false, clearColor: 0x000000 });

    private _canvasElement: HTMLCanvasElement = this._renderer.domElement;

    private static _ORIGIN = new THREE.Vector3(0, 0, 0);

    private _pvObjectGroup = new THREE.Group();

    private _reqAnimFrameHandle: number;

    // private _slicer: microtome.slicer.Slicer = new microtome.slicer.Slicer(this.scene, this._renderer);
    private _slicer: microtome.slicer.AdvancedSlicer = new microtome.slicer.AdvancedSlicer(
        this.scene, 0.1, 0.1, 0.5, 1, 0, this._renderer);

    public disabled: boolean = false;

    public sliceAt: number;

    constructor(private _canvasHome: HTMLDivElement, private scene: PrinterScene) {
        this.attached();
    }

    public attached() {
        // this._canvasElement.className += " fit"
        this._canvasHome.appendChild(this._canvasElement);
        this._startRendering();
    }

    public detached() {
        this._stopRendering();
    }

    disabledChanged(newValue: boolean, oldValue: boolean) {
        if (!newValue) {
            this._startRendering();
        } else {
            this._stopRendering();
        }
    }


    private _stopRendering() {
        if (this._slicer) {
            if (this._reqAnimFrameHandle) {
                window.cancelAnimationFrame(this._reqAnimFrameHandle)
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
        this._renderer.setSize(pvw * scale, pvd * scale);
        canvas.style.width = `${pvw * scale}px`;
        canvas.style.height = `${pvd * scale}px`;
        // TODO fix NEED dirty check on div resize
        this._slicer.sliceAt(this.sliceAt);
        this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
    }
}