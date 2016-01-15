module microtome.three_d {

  var slicer = microtome.slicer;
  var three_d = microtome.three_d;

  /// Controls a slicer providing a interactive preview
  /// Not intended for actual slicing
  export class InteractiveSlicer {
    _slicer: slicer.Slicer = null;

    // Events and interaction
    _enabled: boolean = false;
    //  bool _interactive = false;
    // List<StreamSubscription> _handlerSubscriptions = [];

    // Zoom Control
    _zoomActiveKeyCode: number = null;
    //  bool _zoomActive = null;
    _currZoomSpeed: number = 0.0;
    _zoomStartTime: number = 0;
    //  double _zoomTotalDistance = 0.0;
    maxZoomSpeed: number = 450.0;
    zoomAccelPerS: number = 50.0;
    /// Slice thickness, defaults to 25 microns
    // sliceThickness: number = 0.025;
    minZ: number = 0.0;
    // maxZ: number = 4 * 10 ^ 12;
    sliceZ: number = 0.0;
    _printObjectsContainer: THREE.Object3D = null;
    _printVolume: three_d.PrintVolume = null;
    // recalcZ: boolean = false;
    // _target: HTMLCanvasElement = null;

    constructor(public scene: THREE.Scene, public sliceThickness: number, public maxZ: number = -1.0, public recalcZ: boolean = true) {
      this.scene.children.forEach((child) => {
        if (child instanceof PrintVolume) {
          // print('Got children!');
          this._printVolume = child;
        }
        if (child.name === 'PrintObjects') {
          // print('Got print objects!');
          this._printObjectsContainer = child;
          // window.console.log(_printObjectsContainer);
        }
      });
      this._recalcZ();
      // this._slicer = new slicer.Slicer(this.scene, this.renderer, this._printVolume, this.printObjects);
    }

    _recalcZ() {
      if (this.maxZ === -1.0 || this.recalcZ) {
        var z: number = 0.0;
        this.printObjects.forEach((o3d: THREE.Mesh) => {
          var o3dz = o3d.geometry.boundingBox.max.z;
          if (o3dz > z) {
            z = o3dz;
          }
        });
        this.maxZ = z;
      }
    }

    get printObjects(): THREE.Object3D[] {
      return this._printObjectsContainer.children;
    }

    /// Dispose resources
    // dispose() {
    // _unhookHandlers();
    // _slicer.dispose();
    // }

    //===============================================================================
    // Event Handlers
    // TODO Move to core app
    //===============================================================================

    resized() {
      if (this._enabled) {
        this._slicer.resize();
        this._slicer.sliceAt(this.sliceZ);
      }
    }

    // _hookHandlers() {
    //   _handlerSubscriptions.add(window.onKeyDown.listen(_handleKeyboardEventDown));
    //   _handlerSubscriptions.add(window.onKeyUp.listen(_handleKeyboardEventUp));
    // }
    //
    // _unhookHandlers() {
    //   _handlerSubscriptions.forEach((s) => s.cancel());
    //   _handlerSubscriptions.clear();
    // }

    // _handleKeyboardEventDown(KeyboardEvent kbe) {
    //   KeyEvent ke = new KeyEvent.wrap(kbe);
    //   // ke.repeat currently stupidly unimplemented...
    //   if (ke.shiftKey && (ke.keyCode ===KeyCode.UP || ke.keyCode ===KeyCode.DOWN)) {
    //     if (!kbe.repeat && _zoomActiveKeyCode ===null) {
    //       //print('Zoom START');
    //       _zoomActiveKeyCode = ke.keyCode;
    //       _zoomStartTime = kbe.timeStamp;
    //     }
    //     var sign = -1.0;
    //     if (ke.keyCode ===KeyCode.DOWN) {
    //       sign = 1.0;
    //     }
    //     var t = (kbe.timeStamp - _zoomStartTime) / 1000.0 + 0.25;
    //     _currZoomSpeed = _currZoomSpeed + zoomAccelPerS * t;
    //     if (_currZoomSpeed > maxZoomSpeed) _currZoomSpeed = maxZoomSpeed;
    //     //      var zoomDistance = sign * _currZoomSpeed * t;
    //     //      var zoomDelta = zoomDistance - _zoomTotalDistance;
    //     //      _zoomTotalDistance = zoomDistance;
    //     //print('${kbe.repeat} ${t}: zooming ${zoomDelta} total ${_zoomTotalDistance}');
    //     sliceZ -= sign * sliceThickness;
    //     if (sliceZ < 0.0) sliceZ = 0.0;
    //     if (sliceZ > maxZ - sliceThickness) sliceZ = maxZ;
    //     _slicer.sliceAt(sliceZ);
    //     print('SLICE: ${minZ} ${sliceZ} ${maxZ}');
    //   }
    // }
    //
    // _handleKeyboardEventUp(KeyboardEvent kbe) {
    //   KeyEvent ke = new KeyEvent.wrap(kbe);
    //   //window.console.log(kbe);
    //   if (ke.shiftKey && (ke.keyCode ===_zoomActiveKeyCode)) {
    //     //print('Zoom Stop');
    //     _zoomActiveKeyCode = null;
    //     _currZoomSpeed = 0.0;
    //     _zoomStartTime = 0;
    //     //_zoomTotalDistance = 0.0;
    //   }
    // }

    //==================================================================
    // Properties
    //==================================================================

    get enabled(): boolean {
      return this._enabled;
    }

    set enabled(value: boolean) {
      this._enabled = value;
      if (this._enabled) {
        //_printVolume.visible = false;
        // _hookHandlers();
        this._slicer.setupSlicerPreview();
        this._slicer.resize();
        this._slicer.sliceAt(this.sliceZ);
        this._recalcZ();
      } else {
        //_printVolume.visible = true;
        // _unhookHandlers();
        this._slicer.teardownSlicerPreview();
      }
    }

    get slice(): number {
      return Math.ceil(this.sliceZ / this.sliceThickness);
    }

    get numSlices(): number {
      return Math.ceil(this.maxZ / this.sliceThickness);
    }
  }

}
