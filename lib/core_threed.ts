import * as printer from "./printer_config";
import * as THREE from "three";
import * as mats from "./materials";

export type CameraTarget = THREE.Vector3 | THREE.Mesh | PrintVolumeView;

export class CameraNav {
  _sceneDomElement: HTMLElement;
  _camera: THREE.Camera;
  //   final Logger log = new Logger('camera_nav');
  //
  /// Camera nav enabled?
  _enabled: boolean;
  _target: CameraTarget = new THREE.Vector3(0.0, 0.0, 0.0);
  homePosition: THREE.Vector3 = new THREE.Vector3(0.0, 0.0, 100.0);
  /// Do we follow mouse movements across the whole window?
  useWholeWindow = true;
  /// Is zooming enabled?
  allowZoom = false;
  /// Minimum distance to zoom in to
  minZoomDistance = 5.0;
  /// Maximum distance to zoom out to
  maxZoomDistance = 1000.0;
  /// Restrict rotation in x-y plane
  thetaMin = 0.0;
  /// Restrict rotation in x-y plane
  thetaMax = 0.0;
  /// Min phi angle to +z
  phiMin = 0.0;
  /// Max phi angle to +z
  phiMax = Math.PI;
  maxZoomSpeed = 300.0;
  zoomAccelPerS = 20.0;

  _screenWidth = 0;
  _screenHeight = 0;
  _inSceneDomElement = false;
  _active = false;
  // For rotating the camera about the target
  _startR = 0.0;
  _startTheta = 0.0;
  _startPhi = 0.0;
  // Prevent gimbal lock, we never allow
  // phi to be exactly 0 or PI
  _min_phi_delta = 0.0001;
  _start: THREE.Vector2 = new THREE.Vector2(0.0, 0.0);

  // private zoomActiveKeyCode:KeyCo = null;
  _currZoomSpeed = 0.0;
  _zoomStartTime = 0;
  _zoomTotalDistance = 0.0;

  //   List<StreamSubscription> _handlerSubscriptions = [];

  /**
  * If thetaMin == thetaMax,then free spinning around the z axis is assumed
  * If useWholeWindow is true, then even mouse pointer leaves bounds of sceneDomElement
  * camera will still rotate around scene. if set to false, camera navigation stops
  * when pointer leaves scene element
  * By default, camera target is Vector3(0.0, 0.0, 0.0) in global space
  */
  constructor(camera: THREE.Camera, sceneDomElement: HTMLElement, enabled: boolean = true) {
    this._camera = camera;
    this._sceneDomElement = sceneDomElement;
    this.homePosition = camera.position.clone();
    this.enabled = enabled;
  }

  /**
  * Send the camera to the home position and
  * have it look at the target
  *
  * Home position may be set in constructor or using
  * property of same name. Default value is Vector3 (0.0,0.0,100.0)
  */
  goHome() {
    if (!this.enabled) return;
    this._camera.position = this.homePosition.clone();
    this.lookAtTarget();
  }

  /// Camera is moved to specified position and then
  /// made to look at the current value of target
  goToPosition(position: THREE.Vector3) {
    if (!this.enabled) return;
    this._camera.position = position;
    this.lookAtTarget();
  }

  /// Frame the current target so it all fits in the current
  /// viewport
  ///
  /// If current target is a Object3D then frame it, else
  /// look at it.
  frameTarget() {
    if (!this.enabled) return;
    if (this.target instanceof THREE.Vector3 || this._camera instanceof THREE.OrthographicCamera) {
      this.lookAtTarget();
    }
    else if (this._target instanceof PrintVolumeView) {
      var pv = <PrintVolumeView>this._target;
      var pCamera = <THREE.PerspectiveCamera>this._camera;
      var bb = pv.boundingBox.clone();
      var min = bb.min;
      var max = bb.max;
      var len = Math.abs(max.x - min.x);
      var ylen = Math.abs(max.y - min.y);
      if (ylen > len) len = ylen;
      var zlen = Math.abs(max.z - min.z);
      if (zlen > len) len = zlen;
      var angle = (pCamera.fov / 360.0) * 2.0 * Math.PI;
      var frameDist =
        ((len / 2.0) / Math.sin(angle / 2.0)) * Math.cos(angle / 2.0);
      this.zoomToTarget(frameDist, true);

    } else if (this._target instanceof THREE.Mesh) {
      // TODO, with orthographic camera we could 'zoom in'
      // by recalculating scene bounds but we have no handle
      /** to scene... */
      var mesh = <THREE.Mesh>this._target;
      if (mesh.geometry.boundingBox === null) {
        mesh.geometry.computeBoundingBox();
      }
      var pCamera = <THREE.PerspectiveCamera>this._camera;
      var bb = mesh.geometry.boundingBox.clone();
      var min = bb.min;
      var max = bb.max;
      var len = Math.abs(max.x - min.x);
      var ylen = Math.abs(max.y - min.y);
      if (ylen > len) len = ylen;
      var zlen = Math.abs(max.z - min.z);
      if (zlen > len) len = zlen;
      var angle = (pCamera.fov / 360.0) * 2.0 * Math.PI;
      var frameDist =
        ((len / 2.0) / Math.sin(angle / 2.0)) * Math.cos(angle / 2.0);
      this.zoomToTarget(frameDist, true);
    }
  }

  /// Look at the current set target
  ///
  lookAtTarget() {
    this._camera.lookAt(this._targetPosition());
  }

  /// Move closer to the target by the given amount
  /// Positive zooms in,
  /// Negative zooms out
  /// zoomAmount is treated as relative to current position
  /// unless absolute is true
  zoomToTarget(zoomAmount: number, absolute: boolean = false) {
    if (!this.enabled) return;
    var cameraTargetDelta = this._targetPosition().clone().sub(this._camera.position);
    var vecToTarget = cameraTargetDelta.clone().normalize();
    var distanceToTarget = cameraTargetDelta.length();
    var newCamDistance = distanceToTarget - zoomAmount;
    if (newCamDistance < this.minZoomDistance) {
      zoomAmount = distanceToTarget - this.minZoomDistance;
      // Never perfectly zero, else zoomout breaks
      // because vector to target becomes zero length
      if (this.minZoomDistance == 0) {
        zoomAmount -= 0.001;
      }
    } else if (newCamDistance > this.maxZoomDistance) {
      zoomAmount = distanceToTarget - this.maxZoomDistance;
    }
    this._camera.position.add(vecToTarget.multiplyScalar(zoomAmount));
    this.lookAtTarget();
  }
  //
  rotateTheta(theta: number) {
    this._startRotation();
    this._rotateCamera(theta, 0.0);
  }
  //
  rotatePhi(phi: number) {
    this._startRotation();
    this._rotateCamera(0.0, phi);
  }

  //--------------------------------------------------------------------
  // Handlers
  //--------------------------------------------------------------------

  private _hookHandlers() {
    this._sceneDomElement.addEventListener("mouseenter", this._handleSceneDomElementMouseEnter);
    this._sceneDomElement.addEventListener("mouseleave", this._handleSceneDomElementMouseLeave);
    window.addEventListener("mousemove", this._handleWindowMouseMove)
    window.addEventListener("mousedown", this._handleWindowMouseDown)
    window.addEventListener("mouseup", this._handleWindowMouseUp)
    window.addEventListener("wheel", this._handleWindowMouseScroll)
  }

  private _unhookHandlers() {
    this._sceneDomElement.removeEventListener("mouseenter", this._handleSceneDomElementMouseEnter);
    this._sceneDomElement.removeEventListener("mouseleave", this._handleSceneDomElementMouseLeave);
    window.removeEventListener("mousemove", this._handleWindowMouseMove)
    window.removeEventListener("mousedown", this._handleWindowMouseDown)
    window.removeEventListener("mouseup", this._handleWindowMouseUp)
    window.removeEventListener("wheel", this._handleWindowMouseScroll)
  }

  _handleWindowMouseDown = (e: MouseEvent) => {
    if (e.button == 0 && this._inSceneDomElement) {
      e.preventDefault();
      this._active = true;
      this._start.set(e.screenX, e.screenY);
      this._screenWidth = window.screen.width;
      this._screenHeight = window.screen.height;
      this._startRotation();
      // Using mathematical spherical coordinates
      //print('Start: ${_startR} ${_startTheta} ${_startPhi}');
    }
  }



  _startRotation() {
    var camTargetDelta = (this._camera.position.clone().sub(this._targetPosition()));
    this._startR = camTargetDelta.length();
    camTargetDelta.normalize();
    this._startTheta = Math.atan2(camTargetDelta.y, camTargetDelta.x);
    this._startPhi = Math.acos(camTargetDelta.z);
  }

  _handleSceneDomElementMouseEnter = (e: MouseEvent) => {
    this._inSceneDomElement = true;
  }

  _handleSceneDomElementMouseLeave = (e: MouseEvent) => {
    this._inSceneDomElement = false;
  }

  _handleWindowMouseMove = (e: MouseEvent) => {
    if (this._active && (this._inSceneDomElement || this.useWholeWindow)) {
      var pos = new THREE.Vector2(e.screenX + 0.0, e.screenY + 0.0);
      var distanceX = -(pos.x - this._start.x);
      var distanceY = -(pos.y - this._start.y);
      var deltaTheta = (distanceX / this._screenWidth) * 2.0 * Math.PI;
      // Phi only varies over 180 degrees or 1 pi radians
      var deltaPhi = (distanceY / this._screenHeight) * Math.PI;
      this._rotateCamera(deltaTheta, deltaPhi);
    }
  }

  _handleWindowMouseScroll = (e: WheelEvent) => {
    if (!this._inSceneDomElement) return;
    // Shift key changes scroll axis in chrome
    if (e.deltaY > 0 || e.deltaX > 0) {
      this.zoomToTarget(-10.0);
    } else if (e.deltaY < 0 || e.deltaX < 0) {
      this.zoomToTarget(10.0);
    }
  }

  _rotateCamera(deltaTheta: number, deltaPhi: number) {
    var theta = this._startTheta + deltaTheta;
    // Phi only varies over 180 degrees or 1 pi radians
    var phi = this._startPhi + deltaPhi;
    if (phi < this.phiMin) {
      phi = this.phiMin;
    } else if (phi > this.phiMax) {
      phi = this.phiMax;
    }
    if (phi <= 0) phi = this._min_phi_delta;
    if (phi >= Math.PI) phi = Math.PI - this._min_phi_delta;
    var x = this._startR * Math.sin(phi) * Math.cos(theta);
    var y = this._startR * Math.sin(phi) * Math.sin(theta);
    var z = this._startR * Math.cos(phi);
    var tp = this._targetPosition();
    this._camera.position.set(x + tp.x, y + tp.y, z + tp.z);
    this.lookAtTarget();
  }

  _handleWindowMouseUp = (e: MouseEvent) => {
    if (e.button == 0) {
      this._active = false;
    }
  }

  _targetPosition(): THREE.Vector3 {
    if (this._target instanceof THREE.Vector3) {
      return <THREE.Vector3>this._target;
    }
    if (this._target instanceof THREE.Mesh) {
      return (<THREE.Mesh>this._target).position;
    }
    return new THREE.Vector3(0, 0, 0);
  }

  get target(): CameraTarget {
    return this._target;
  }

  set target(newTarget: CameraTarget) {
    this._target = newTarget;
    this.lookAtTarget();
  }

  get enabled(): boolean {
    return this._enabled;
  }

  set enabled(val: boolean) {
    this._enabled = val;
    if (this._enabled) {
      this._hookHandlers();
    } else {
      this._unhookHandlers();
    }
  }
}


/**
* Utility class for displaying print volume
* All dimensions are in mm
* R-G-B => X-Y-Z
*/
export class PrintVolumeView extends THREE.Group {
  private _bbox: THREE.Box3;

  constructor(width: number, depth: number, height: number) {
    super();
    this.scale.set(width, depth, height);
    this._recalcBBox();
    // this.add(this._pvGroup);
    var planeGeom: THREE.PlaneGeometry = new THREE.PlaneGeometry(1.0, 1.0);
    var planeMaterial = mats.CoreMaterialsFactory.whiteMaterial.clone();
    planeMaterial.side = THREE.DoubleSide;
    var bed = new THREE.Mesh(planeGeom, planeMaterial);
    this.add(bed);

    var xlinesPts = [
      new THREE.Vector3(-0.5, 0.5, 0.0),
      new THREE.Vector3(0.5, 0.5, 0.0),
      new THREE.Vector3(-0.5, -0.5, 0.0),
      new THREE.Vector3(0.5, -0.5, 0.0)
    ];
    var xlineGeometry = new THREE.Geometry();
    xlineGeometry.vertices = xlinesPts;
    var xLines1 = new THREE.LineSegments(xlineGeometry.clone(),
      mats.CoreMaterialsFactory.xLineMaterial);
    this.add(xLines1);
    var xLines2 = new THREE.LineSegments(xlineGeometry.clone(),
      mats.CoreMaterialsFactory.xLineMaterial);
    xLines2.position.set(0.0, 0.0, 1.0);
    this.add(xLines2);

    var ylinesPts = [
      new THREE.Vector3(0.5, 0.5, 0.0),
      new THREE.Vector3(0.5, -0.5, 0.0),
      new THREE.Vector3(-0.5, -0.5, 0.0),
      new THREE.Vector3(-0.5, 0.5, 0.0)
    ];
    var ylineGeometry = new THREE.Geometry();
    ylineGeometry.vertices = ylinesPts;
    var yLines1 = new THREE.LineSegments(ylineGeometry.clone(),
      mats.CoreMaterialsFactory.yLineMaterial);
    this.add(yLines1);
    var yLines2 = new THREE.LineSegments(ylineGeometry.clone(),
      mats.CoreMaterialsFactory.yLineMaterial);
    yLines2.position.set(0.0, 0.0, 1.0);
    this.add(yLines2);

    var zlinesPts = [
      new THREE.Vector3(0.5, 0.5, 0.0),
      new THREE.Vector3(0.5, 0.5, 1.0),
      new THREE.Vector3(-0.5, 0.5, 0.0),
      new THREE.Vector3(-0.5, 0.5, 1.0)
    ];
    var zlineGeometry = new THREE.Geometry();
    zlineGeometry.vertices = zlinesPts;
    var zLines1 = new THREE.LineSegments(zlineGeometry.clone(),
      mats.CoreMaterialsFactory.zLineMaterial);
    this.add(zLines1);
    var zLines2 = new THREE.LineSegments(zlineGeometry.clone(),
      mats.CoreMaterialsFactory.zLineMaterial);
    zLines2.position.set(0.0, -1.0, 0.0);
    this.add(zLines2);
  }

  resize(pv: printer.PrintVolume): void
  resize(width: number, depth: number, height: number): void
  resize(widthOrPv: number | printer.PrintVolume, depth?: number, height?: number): void {
    if (typeof widthOrPv == "number") {
      this.scale.set(widthOrPv as number, depth, height);
    } else {
      var pv = widthOrPv as printer.PrintVolume
      this.scale.set(pv.width_mm, pv.depth_mm, pv.height_mm)
    }
    this._recalcBBox();
  }

  private _recalcBBox(): void {
    var halfWidth = this.scale.x / 2.0;
    var halfDepth = this.scale.y / 2.0;
    var min = new THREE.Vector3(-halfWidth, -halfDepth, 0.0);
    var max = new THREE.Vector3(halfWidth, halfDepth, this.scale.z);
    this._bbox = new THREE.Box3(min, max);
  }

  get boundingBox(): THREE.Box3 {
    return this._bbox;
  }

  get width(): number {
    return this.scale.x;
  }

  get depth(): number {
    return this.scale.y;
  }

  get height(): number {
    return this.scale.z;
  }

  // /**
  // * Set up print volume for slicing if enable is
  // * true, otherwise set it up to display the printvolume
  // * normally
  // */
  // public prepareForSlicing(enable: boolean) {
  //   this._pvGroup.visible = !enable;
  //   this._sliceBackground.visible = enable;
  // }

}


/**
* Subclass of THREE.Scene with several convenience methods
*/
export class PrinterScene extends THREE.Scene {

  private _printVolume: PrintVolumeView;
  private _printObjectsHolder: THREE.Group;
  private _printObjects: THREE.Mesh[];

  constructor() {
    super();
    this._printVolume = new PrintVolumeView(100, 100, 100);
    this.add(this._printVolume);
    this._printObjectsHolder = new THREE.Group();
    this.add(this._printObjectsHolder);
    this._printObjects = this._printObjectsHolder.children as THREE.Mesh[];
  }

  get printObjects(): THREE.Mesh[] {
    return this._printObjects;
  }

  get printVolume(): PrintVolumeView {
    return this._printVolume;
  }

  public removePrintObject(child: THREE.Object3D) {
    this._printObjectsHolder.remove(child);
  }

  public hidePrintObjects() {
    this._printObjectsHolder.visible = false;
  }

  public showPrintObjects() {
    this._printObjectsHolder.visible = true;
  }
}

// TODO Turn into extension method
export class PrintMesh extends THREE.Mesh {

  private _gvolume: number = null;

  constructor(geometry?: THREE.Geometry, material?: THREE.Material | THREE.Material[]) {
    super(geometry, material);
    this._calculateVolume();
  }

  public static fromMesh(mesh: THREE.Mesh) {
    var geom: THREE.Geometry;
    if (mesh.geometry instanceof THREE.BufferGeometry) {
      geom = new THREE.Geometry().fromBufferGeometry(<THREE.BufferGeometry>mesh.geometry);
    } else {
      geom = <THREE.Geometry>mesh.geometry
    }
    return new PrintMesh(geom, mesh.material);
  }


  /**
  * Gets the volume of the mesh. Only works if Geometry is
  * PrintGeometry, else returns null;
  */
  public get volume(): number {
    // The true volume is the geom volume multiplied by the scale factors
    return this._gvolume * (this.scale.x * this.scale.y * this.scale.z);
  }


  private _calculateVolume() {
    let geom: THREE.Geometry = <THREE.Geometry>this.geometry
    var faces = geom.faces;
    var vertices = geom.vertices;

    var face: THREE.Face3;
    var v1: THREE.Vector3;
    var v2: THREE.Vector3;
    var v3: THREE.Vector3;

    for (var i = 0; i < faces.length; i++) {
      face = faces[i];

      v1 = vertices[face.a];
      v2 = vertices[face.b];
      v3 = vertices[face.c];
      this._gvolume += (
        -(v3.x * v2.y * v1.z)
        + (v2.x * v3.y * v1.z)
        + (v3.x * v1.y * v2.z)
        - (v1.x * v3.y * v2.z)
        - (v2.x * v1.y * v3.z)
        + (v1.x * v2.y * v3.z)
      ) / 6;
    }
  }
}
