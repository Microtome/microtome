
@component("pv-view")
class PrinterVolumeView extends polymer.Base {

  private _renderer: THREE.Renderer = new THREE.WebGLRenderer({ alpha: true, antialias: true, clearColor: 0x000000, clearAlpha: 0 });

  private _raycaster: THREE.Raycaster = new THREE.Raycaster();

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

  //--------------------------------------------------------
  // Properties
  //--------------------------------------------------------

  @property({})
  public scene: microtome.three_d.PrinterScene;

  @property({ notify: true, readOnly: false, reflectToAttribute: true, type: Boolean })
  public disabled: boolean = false;

  @property({ notify: true, readOnly: false })
  public scatterColor: string = "#777777"

  @property({ notify: true, readOnly: false })
  public skyColor: string = "#AACCFF"

  @property({ notify: true, readOnly: false })
  public groundColor: string = "#775533"

  @property({ notify: true, readOnly: false, type: Object })
  public pickedMesh: THREE.Mesh = null;

  @property({ notify: true, readOnly: false, type: String })
  public _rotX: string;

  @property({ notify: true, readOnly: false, type: String })
  public _rotY: string;

  @property({ notify: true, readOnly: false, type: String })
  public _rotZ: string;

  @property({ notify: true, readOnly: false, type: String })
  public _sX: string;

  @property({ notify: true, readOnly: false, type: String })
  public _sY: string;

  @property({ notify: true, readOnly: false, type: String })
  public _sZ: string;


  //-----------------------------------------------------------
  // Observers
  //-----------------------------------------------------------

  @observe("disabled")
  disabledChanged(newValue: boolean, oldValue: boolean) {
    if (!newValue) {
      this.async(this._startRendering.bind(this), 100)
    } else {
      this._stopRendering();
    }
    if (this._camNav) {
      this._camNav.enabled = !newValue;
    }
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

  @observe("pickedMesh")
  pickedMeshChanged(newMesh: THREE.Mesh, oldMesh: THREE.Mesh) {
    if (newMesh) {
      var rotation = newMesh.rotation;
      this._rotX = (((rotation.x / (2 * Math.PI)) * 360) % 360).toFixed(0);
      this._rotY = (((rotation.y / (2 * Math.PI)) * 360) % 360).toFixed(0);
      this._rotZ = (((rotation.z / (2 * Math.PI)) * 360) % 360).toFixed(0);
      var scale = newMesh.scale;
      this._sX = scale.x.toFixed(2);
      this._sY = scale.y.toFixed(2);
      this._sZ = scale.z.toFixed(2);
    } else {
      this._rotX = this._rotY = this._rotZ = null;
      this._sX = this._sY = this._sZ = null;
    }
  }

  @observe("_rotX,_rotY,_rotZ")
  rotationChanged(rotX: string, rotY: string, rotZ: string) {
    if (!this.pickedMesh) return;
    if (rotX) {
      this.pickedMesh.rotation.x = (parseFloat(rotX) / 360) * 2 * Math.PI;
    }
    if (rotY) {
      this.pickedMesh.rotation.y = (parseFloat(rotY) / 360) * 2 * Math.PI;
    }
    if (rotZ) {
      this.pickedMesh.rotation.z = (parseFloat(rotZ) / 360) * 2 * Math.PI;
    }
  }

  @observe("_sX,_sY,_sZ")
  scaleChanged(sX: string, sY: string, sZ: string) {
    if (!this.pickedMesh) return;
    if (sX) {
      this.pickedMesh.scale.x = parseFloat(sX);
    }
    if (sY) {
      this.pickedMesh.scale.y = parseFloat(sY);
    }
    if (sZ) {
      this.pickedMesh.scale.z = parseFloat(sZ);
    }
  }

  //----------------------------------------------------------
  // Lifecycle methods
  //----------------------------------------------------------

  public attached() {
    this._canvasHome = this.$["pv-canvas-home"] as HTMLDivElement;
    this._canvasHome.appendChild(this._canvasElement);
    this._pvCamera.up.set(0, 0, 1);
    this._pvCamera.position.set(0, 350, 250);
    this._configureLighting();
    this._pvCamera.lookAt(this._printerVolume.position);
    this._camNav = new microtome.three_d.CameraNav(this._pvCamera, this._canvasElement, true)
    this._camNav.target = this._printerVolume;
    this._camNav.frameTarget();
    this._startRendering();
  }

  public detached() {
    this._stopRendering();
  }

  private _configureLighting() {
    this._scatterLight.color.setStyle(this.scatterColor);
    this.scene.add(this._scatterLight);
    this._skyLight.color.setStyle(this.skyColor);
    this._skyLight.intensity = 0.65;
    this._skyLight.position.set(0, 0, 1000);
    this.scene.add(this._skyLight);
    this._groundLight.color.setStyle(this.groundColor);
    this._groundLight.intensity = 0.45;
    this._groundLight.position.set(0, 0, -1000);
    this.scene.add(this._groundLight);
  }

  //---------------------------------------------------------
  // Rendering lifecycle hooks
  //---------------------------------------------------------

  private _stopRendering() {
    if (this._reqAnimFrameHandle) window.cancelAnimationFrame(this._reqAnimFrameHandle)
  }

  private _startRendering() {
    if (this._reqAnimFrameHandle) window.cancelAnimationFrame(this._reqAnimFrameHandle);
    this._reqAnimFrameHandle = window.requestAnimationFrame(this._render.bind(this));
  }

  private _render(timestamp: number) {
    var canvas = this._canvasElement;
    var div = this._canvasHome
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

  //---------------------------------------------------------
  // Picking support
  //---------------------------------------------------------

  private _mouseXY = new THREE.Vector2();

  private _doPick = false;

  public preparePick(e: MouseEvent) {
    if (e.buttons === 1) {
      this._doPick = true;
    }
  }

  public cancelPick(e: MouseEvent) {
    this._doPick = false;
  }

  public tryPick(e: MouseEvent) {
    if (this._doPick) {
      var bounds = this._canvasHome.getBoundingClientRect();
      var x = (e.clientX / bounds.width) * 2 - 1;
      var y = - (e.clientY / bounds.height) * 2 + 1;
      // update the picking ray with the camera and mouse position
      this._mouseXY.x = x;
      this._mouseXY.y = y;
      this._raycaster.setFromCamera(this._mouseXY, this._pvCamera);
      // calculate objects intersecting the picking ray
      var intersects = this._raycaster.intersectObjects(this.scene.printObjects);
      if (intersects.length > 0) {
        if (this.pickedMesh != null) {
          this.pickedMesh.material = microtome.three_d.CoreMaterialsFactory.objectMaterial;
          this.pickedMesh = null;
        }
        this.pickedMesh = intersects[0].object as THREE.Mesh;
        this.pickedMesh.material = microtome.three_d.CoreMaterialsFactory.selectMaterial;
      }
    }
  }
}

PrinterVolumeView.register();
