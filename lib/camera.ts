import * as THREE from "three";

import {PrintVolumeView} from "./printer";

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
