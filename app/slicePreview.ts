import * as microtome from "microtome"
import * as THREE from "three"

type PrinterScene = microtome.three_d.PrinterScene;

/**
 * The slice preview previews slices
 */
export class SlicePreview {

    private static _ORIGIN = new THREE.Vector3(0, 0, 0);

    private _pvObjectGroup = new THREE.Group();

    private _reqAnimFrameHandle: number;

    private _slicer: microtome.slicer.AdvancedSlicer 

    public disabled: boolean = false;

    public sliceAt: number;

    constructor(private _canvasHome: HTMLDivElement, private scene: PrinterScene) {
        this.attached();
        this._slicer = new microtome.slicer.AdvancedSlicer(
            this.scene, 0.1, 0.1, 1.5, 2.5, 0, _canvasHome);
    }

    public attached() {
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
        var div = this._canvasHome
        var pvw = this.scene.printVolume.width;
        var pvd = this.scene.printVolume.depth;
        var scaleh = div.clientHeight / pvd;
        var scalew = div.clientWidth / pvw;
        
        var scale = scaleh < scalew ? scaleh : scalew;
        this._slicer.setSize(pvw * scale, pvd * scale);;
        // TODO fix NEED dirty check on div resize
        this._slicer.sliceAt(this.sliceAt);
        this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
    }
}