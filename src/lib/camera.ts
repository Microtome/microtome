/**
 * This module contains camera navigation related classes.
 */

import * as THREE from "three";

import {PrintVolumeView} from "./printer";

export type CameraTarget = THREE.Vector3 | THREE.Mesh | PrintVolumeView;

const MIN_PHI_DELTA = 0.0001;

export class CameraNav {
  public sceneDomElement: HTMLElement;
  public camera: THREE.Camera;
  /// Camera nav enabled?
  public homePosition: THREE.Vector3 = new THREE.Vector3(0.0, 0.0, 100.0);
  /// Do we follow mouse movements across the whole window?
  public useWholeWindow = true;
  /// Is zooming enabled?
  public allowZoom = false;
  /// Minimum distance to zoom in to
  public minZoomDistance = 5.0;
  /// Maximum distance to zoom out to
  public maxZoomDistance = 1000.0;
  /// Restrict rotation in x-y plane
  public thetaMin = 0.0;
  /// Restrict rotation in x-y plane
  public thetaMax = 0.0;
  /// Min phi angle to +z
  public phiMin = 0.0;
  /// Max phi angle to +z
  public phiMax = Math.PI;
  public maxZoomSpeed = 300.0;
  public zoomAccelPerS = 20.0;

  public screenWidth = 0;
  public screenHeight = 0;
  public inSceneDomElement = false;
  public active = false;
  // For rotating the camera about the target
  public startR = 0.0;
  public startTheta = 0.0;
  public startPhi = 0.0;
  // Prevent gimbal lock, we never allow
  // phi to be exactly 0 or PI
  public start: THREE.Vector2 = new THREE.Vector2(0.0, 0.0);

  // private zoomActiveKeyCode:KeyCo = null;
  public currZoomSpeed = 0.0;
  public zoomStartTime = 0;
  public zoomTotalDistance = 0.0;

  private _enabled: boolean;
  private _target: CameraTarget = new THREE.Vector3(0.0, 0.0, 0.0);

  //   List<StreamSubscription> handlerSubscriptions = [];

  /**
   * If thetaMin == thetaMax,then free spinning around the z axis is assumed
   * If useWholeWindow is true, then even mouse pointer leaves bounds of sceneDomElement
   * camera will still rotate around scene. if set to false, camera navigation stops
   * when pointer leaves scene element
   * By default, camera target is Vector3(0.0, 0.0, 0.0) in global space
   */
  constructor(camera: THREE.Camera, sceneDomElement: HTMLElement, enabled: boolean = true) {
    this.camera = camera;
    this.sceneDomElement = sceneDomElement;
    this.homePosition = camera.position.clone();
    this._enabled = enabled;
  }

  /**
   * Send the camera to the home position and
   * have it look at the target
   *
   * Home position may be set in constructor or using
   * property of same name. Default value is Vector3 (0.0,0.0,100.0)
   */
  public goHome() {
    if (!this._enabled) { return; }
    this.camera.position = this.homePosition.clone();
    this.lookAtTarget();
  }

  /// Camera is moved to specified position and then
  /// made to look at the current value of target
  public goToPosition(position: THREE.Vector3) {
    if (!this._enabled) { return; }
    this.camera.position = position;
    this.lookAtTarget();
  }

  /// Frame the current target so it all fits in the current
  /// viewport
  ///
  /// If current target is a Object3D then frame it, else
  /// look at it.
  public frameTarget() {
    if (!this._enabled) { return; }
    if (this._target instanceof THREE.Vector3 || this.camera instanceof THREE.OrthographicCamera) {
      this.lookAtTarget();
    } else if (this._target instanceof PrintVolumeView) {
      const pv = this._target as PrintVolumeView;
      const pCamera = this.camera as THREE.PerspectiveCamera;
      const bb = pv.boundingBox.clone();
      const min = bb.min;
      const max = bb.max;
      let len = Math.abs(max.x - min.x);
      const ylen = Math.abs(max.y - min.y);
      if (ylen > len) { len = ylen; }
      const zlen = Math.abs(max.z - min.z);
      if (zlen > len) { len = zlen; }
      const angle = (pCamera.fov / 360.0) * 2.0 * Math.PI;
      const frameDist =
        ((len / 2.0) / Math.sin(angle / 2.0)) * Math.cos(angle / 2.0);
      this.zoomToTarget(frameDist, true);

    } else if (this._target instanceof THREE.Mesh) {
      // TODO, with orthographic camera we could 'zoom in'
      // by recalculating scene bounds but we have no handle
      /** to scene... */
      const mesh = this._target as THREE.Mesh;
      if (mesh.geometry.boundingBox === null) {
        mesh.geometry.computeBoundingBox();
      }
      const pCamera = this.camera as THREE.PerspectiveCamera;
      const bb = mesh.geometry.boundingBox.clone();
      const min = bb.min;
      const max = bb.max;
      let len = Math.abs(max.x - min.x);
      const ylen = Math.abs(max.y - min.y);
      if (ylen > len) { len = ylen; }
      const zlen = Math.abs(max.z - min.z);
      if (zlen > len) { len = zlen; }
      const angle = (pCamera.fov / 360.0) * 2.0 * Math.PI;
      const frameDist =
        ((len / 2.0) / Math.sin(angle / 2.0)) * Math.cos(angle / 2.0);
      this.zoomToTarget(frameDist, true);
    }
  }

  /// Look at the current set target
  ///
  public lookAtTarget() {
    this.camera.lookAt(this.targetPosition());
  }

  /// Move closer to the target by the given amount
  /// Positive zooms in,
  /// Negative zooms out
  /// zoomAmount is treated as relative to current position
  /// unless absolute is true
  public zoomToTarget(zoomAmount: number, absolute: boolean = false) {
    if (!this._enabled) { return; }
    const cameraTargetDelta = this.targetPosition().clone().sub(this.camera.position);
    const vecToTarget = cameraTargetDelta.clone().normalize();
    const distanceToTarget = cameraTargetDelta.length();
    const newCamDistance = distanceToTarget - zoomAmount;
    if (newCamDistance < this.minZoomDistance) {
      zoomAmount = distanceToTarget - this.minZoomDistance;
      // Never perfectly zero, else zoomout breaks
      // because vector to target becomes zero length
      if (this.minZoomDistance === 0) {
        zoomAmount -= 0.001;
      }
    } else if (newCamDistance > this.maxZoomDistance) {
      zoomAmount = distanceToTarget - this.maxZoomDistance;
    }
    this.camera.position.add(vecToTarget.multiplyScalar(zoomAmount));
    this.lookAtTarget();
  }
  //
  public rotateTheta(theta: number) {
    this.startRotation();
    this.rotateCamera(theta, 0.0);
  }
  //
  public rotatePhi(phi: number) {
    this.startRotation();
    this.rotateCamera(0.0, phi);
  }

  // --------------------------------------------------------------------
  // Handlers
  // --------------------------------------------------------------------

  private hookHandlers() {
    this.sceneDomElement.addEventListener("mouseenter", this.handleSceneDomElementMouseEnter);
    this.sceneDomElement.addEventListener("mouseleave", this.handleSceneDomElementMouseLeave);
    window.addEventListener("mousemove", this.handleWindowMouseMove);
    window.addEventListener("mousedown", this.handleWindowMouseDown);
    window.addEventListener("mouseup", this.handleWindowMouseUp);
    window.addEventListener("wheel", this.handleWindowMouseScroll);
  }

  private unhookHandlers() {
    this.sceneDomElement.removeEventListener("mouseenter", this.handleSceneDomElementMouseEnter);
    this.sceneDomElement.removeEventListener("mouseleave", this.handleSceneDomElementMouseLeave);
    window.removeEventListener("mousemove", this.handleWindowMouseMove);
    window.removeEventListener("mousedown", this.handleWindowMouseDown);
    window.removeEventListener("mouseup", this.handleWindowMouseUp);
    window.removeEventListener("wheel", this.handleWindowMouseScroll);
  }

  private handleWindowMouseDown = (e: MouseEvent) => {
    if (e.button === 0 && this.inSceneDomElement) {
      e.preventDefault();
      this.active = true;
      this.start.set(e.screenX, e.screenY);
      this.screenWidth = window.screen.width;
      this.screenHeight = window.screen.height;
      this.startRotation();
      // Using mathematical spherical coordinates
      // print('Start: ${_startR} ${_startTheta} ${_startPhi}');
    }
  }

  private startRotation() {
    const camTargetDelta = (this.camera.position.clone().sub(this.targetPosition()));
    this.startR = camTargetDelta.length();
    camTargetDelta.normalize();
    this.startTheta = Math.atan2(camTargetDelta.y, camTargetDelta.x);
    this.startPhi = Math.acos(camTargetDelta.z);
  }

  private handleSceneDomElementMouseEnter = (e: MouseEvent) => {
    this.inSceneDomElement = true;
  }

  private handleSceneDomElementMouseLeave = (e: MouseEvent) => {
    this.inSceneDomElement = false;
  }

  private handleWindowMouseMove = (e: MouseEvent) => {
    if (this.active && (this.inSceneDomElement || this.useWholeWindow)) {
      const pos = new THREE.Vector2(e.screenX + 0.0, e.screenY + 0.0);
      const distanceX = -(pos.x - this.start.x);
      const distanceY = -(pos.y - this.start.y);
      const deltaTheta = (distanceX / this.screenWidth) * 2.0 * Math.PI;
      // Phi only varies over 180 degrees or 1 pi radians
      const deltaPhi = (distanceY / this.screenHeight) * Math.PI;
      this.rotateCamera(deltaTheta, deltaPhi);
    }
  }

  private handleWindowMouseScroll = (e: WheelEvent) => {
    if (!this.inSceneDomElement) { return; }
    // Shift key changes scroll axis in chrome
    if (e.deltaY > 0 || e.deltaX > 0) {
      this.zoomToTarget(-10.0);
    } else if (e.deltaY < 0 || e.deltaX < 0) {
      this.zoomToTarget(10.0);
    }
  }

  private rotateCamera(deltaTheta: number, deltaPhi: number) {
    const theta = this.startTheta + deltaTheta;
    // Phi only varies over 180 degrees or 1 pi radians
    let phi = this.startPhi + deltaPhi;
    if (phi < this.phiMin) {
      phi = this.phiMin;
    } else if (phi > this.phiMax) {
      phi = this.phiMax;
    }
    if (phi <= 0) { phi = MIN_PHI_DELTA; }
    if (phi >= Math.PI) { phi = Math.PI - MIN_PHI_DELTA; }
    const x = this.startR * Math.sin(phi) * Math.cos(theta);
    const y = this.startR * Math.sin(phi) * Math.sin(theta);
    const z = this.startR * Math.cos(phi);
    const tp = this.targetPosition();
    this.camera.position.set(x + tp.x, y + tp.y, z + tp.z);
    this.lookAtTarget();
  }

  private handleWindowMouseUp = (e: MouseEvent) => {
    if (e.button === 0) {
      this.active = false;
    }
  }

  private targetPosition(): THREE.Vector3 {
    if (this._target instanceof THREE.Vector3) {
      return this._target as THREE.Vector3;
    }
    if (this._target instanceof THREE.Mesh) {
      return (this._target as THREE.Mesh).position;
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
      this.hookHandlers();
    } else {
      this.unhookHandlers();
    }
  }
}
