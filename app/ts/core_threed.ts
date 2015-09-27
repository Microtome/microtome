// module microtome.three_d {
//
//   // part "camera_nav.dart";
//   // part "slicer_camera_nav.dart";
//   // part "print_volume.dart";
//   // part "shared_renderer.dart";
//
//   /// promote material reuse in a context
//   /// A new material factory should be created and used
//   /// for each webgl renderer or binding errors may occur
//   ///
//   /// Properly encoding 32 bit float in rgba from here:
//   /// http://www.gamedev.net/topic/486847-encoding-16-and-32-bit-floating-point-value-into-rgba-byte-texture/
//   export class CoreMaterialsFactory {
//     static _basicVertex = `
// void main(void) {
//    // compute position
//    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
// }`;
//
//     static _depthShaderFrag = `
//
//     vec4 pack( const in float depth ) {
//         const float toFixed = 255.0/256.0;
//         vec4 result = vec4(0);
//         result.r = fract(depth*toFixed*1.0);
//         result.g = fract(depth*toFixed*255.0);
//         result.b = fract(depth*toFixed*255.0*255.0);
//         result.a = fract(depth*toFixed*255.0*255.0*255.0);
//         return result;
//     }
//
//     void main() {
//       gl_FragColor = pack(gl_FragCoord.z);
//     }`;
//
//     static _sliceShaderFrag = `
// // current slice height in device coordinates
// uniform float cutoff;
//
// // Texture containing depth info of scene
// // packed into rgba
// // Must be same size as current viewport
// uniform sampler2D dTex;
//
// // Since depth conversion not exact
// // Use epsilon for compare
// // 1/2 slice height should be good.
// uniform float epsilon;
//
// uniform int viewWidth;
// uniform int viewHeight;
//
// float unpack(const in vec4 color) {
//   const float fromFixed = 256.0/255.0;
//   float result = color.r*fromFixed/(1.0)
//   +color.g*fromFixed/(255.0)
//   +color.b*fromFixed/(255.*255.0)
//   +color.a*fromFixed/(255.0*255.0*255.0);
//   return result;
// }
//
// bool isInLattice(const in vec4 fragCoord, const in float cutOff){
//   float modX = abs(mod(fragCoord.x, 20.0));
//   float modY = abs(mod(fragCoord.y, 20.0));
//   float modZ = abs(mod(cutOff * 1000.0, 20.0));
//
//   return ( (modX <= 1.0 && modY <= 1.0) || (modX <= 1.0 && modZ <= 10.0) || (modY <= 1.0 && modZ <= 10.0)  );
// }
//
// void main(void) {
//   vec2 lookup = gl_FragCoord.xy / vec2(viewWidth, viewHeight );
//   float depth = unpack(texture2D(dTex, lookup));
//
//   float zCutoff = 1.0 - cutoff;
//
//   if ( gl_FragCoord.z < zCutoff ){
//     discard;
//   }
//
//   if(gl_FrontFacing){
//     float modX = abs(mod(gl_FragCoord.x, 20.0));
//     float modY = abs(mod(gl_FragCoord.y, 20.0));
//     float modZ = abs(mod(gl_FragCoord.z * 20.0 , 20.0));
//     gl_FragColor = vec4(vec3(0), 1);
//     if( isInLattice(gl_FragCoord, zCutoff)
//       && gl_FragCoord.z > epsilon + depth && zCutoff > depth  ){
//       gl_FragColor = vec4(vec3(1), 1);
//     }
//   } else {
//     gl_FragColor = vec4(vec3(1),1);
//   }
// }`;
//
//     static xLineMaterial = new THREE.LineBasicMaterial({ color: 0xd50000, linewidth: 2 });
//     static yLineMaterial = new THREE.LineBasicMaterial({ color: 0x00c853, linewidth: 2 });
//     static zLineMaterial = new THREE.LineBasicMaterial({ color: 0x2962ff, linewidth: 2 });
//     static bBoxMaterial = new THREE.LineBasicMaterial({ color: 0x4fc3f7, linewidth: 2 });
//     static whiteMaterial = new THREE.MeshLambertMaterial({ color: 0xf5f5f5, side: THREE.DoubleSide });
//     static objectMaterial = new THREE.MeshPhongMaterial({ color: 0xCFCFCF, side: THREE.DoubleSide });//, ambient:0xcfcfcf});
//     static selectMaterial = new THREE.MeshPhongMaterial({ color: 0x00CFCF, side: THREE.DoubleSide });//, ambient:0x00cfcf});
//     /**
//     Material for rendering depth textures
//     */
//     static depthMaterial = new THREE.ShaderMaterial({
//       fragmentShader: CoreMaterialsFactory._depthShaderFrag, vertexShader: CoreMaterialsFactory._basicVertex, blending: THREE.NoBlending
//     });
//     /**
//     Material for slicing
//     */
//     static sliceMaterial = new THREE.ShaderMaterial({
//       fragmentShader: CoreMaterialsFactory._sliceShaderFrag,
//       vertexShader: CoreMaterialsFactory._basicVertex,
//       side: THREE.DoubleSide,
//       blending: THREE.NoBlending,
//       uniforms: { 'cutoff': 0.00 }
//     });
//   }
//
//
//   export class CameraNav {
//     private sceneDomElement: HTMLElement;
//     private camera: THREE.Camera;
//     //   final Logger log = new Logger("camera_nav");
//     //
//     /// Camera nav enabled?
//     private _enabled: boolean;
//     private _target: THREE.Vector3 | THREE.Object3D = new THREE.Vector3(0.0, 0.0, 0.0);
//     homePosition = new THREE.Vector3(0.0, 0.0, 100.0);
//     /// Do we follow mouse movements across the whole window?
//     useWholeWindow = true;
//     /// Is zooming enabled?
//     allowZoom = false;
//     /// Minimum distance to zoom in to
//     minZoomDistance = 0.0;
//     /// Maximum distance to zoom out to
//     maxZoomDistance = 1000.0;
//     /// Restrict rotation in x-y plane
//     thetaMin = 0.0;
//     /// Restrict rotation in x-y plane
//     thetaMax = 0.0;
//     /// Min phi angle to +z
//     phiMin = 0.0;
//     /// Max phi angle to +z
//     phiMax = Math.PI;
//     maxZoomSpeed = 300.0;
//     zoomAccelPerS = 20.0;
//
//     private screenWidth = 0;
//     private screenHeight = 0;
//     private inSceneDomElement = false;
//     private active = false;
//     // For rotating the camera about the target
//     private startR = 0.0;
//     private startTheta = 0.0;
//     private startPhi = 0.0;
//     // Prevent gimbal lock, we never allow
//     // phi to be exactly 0 or PI
//     private min_phi_delta = 0.0001;
//     private start = new THREE.Vector2(0.0, 0.0);
//
//     // private zoomActiveKeyCode:KeyCo = null;
//     private currZoomSpeed = 0.0;
//     private zoomStartTime = 0;
//     private zoomTotalDistance = 0.0;
//     //
//     //   List<StreamSubscription> _handlerSubscriptions = [];
//     //
//     /// If thetaMin == thetaMax,then free spinning around the z axis is assumed
//     /// If useWholeWindow is true, then even mouse pointer leaves bounds of sceneDomElement
//     /// camera will still rotate around scene. if set to false, camera navigation stops
//     /// when pointer leaves scene element
//     ///
//     /// By default, camera target is Vector3(0.0, 0.0, 0.0) in global space
//     constructor(camera: THREE.Camera, sceneDomElement: HTMLElement, enabled: boolean = true) {
//       this.camera = camera;
//       this.sceneDomElement = sceneDomElement;
//       this.homePosition = camera.position.clone();
//       this.enabled = enabled;
//     }
//
//     /// Send the camera to the home position and
//     /// have it look at the target
//     ///
//     /// Home position may be set in constructor or using
//     /// property of same name. Default value is Vector3 (0.0,0.0,100.0)
//     ///
//     goHome() {
//       if (!this.enabled) return;
//       this.camera.position = this.homePosition.clone();
//       lookAtTarget();
//     }
//
//       /// Camera is moved to specified position and then
//       /// made to look at the current value of target
//       goToPosition(position:THREE.Vector3 ) {
//         if (!this.enabled) return;
//         this.camera.position = position;
//         lookAtTarget();
//       }
//
//       /// Frame the current target so it all fits in the current
//       /// viewport
//       ///
//       /// If current target is a Object3D then frame it, else
//       /// look at it.
//       frameTarget() {
//         if (!this.enabled) return;
//         if (this._target instanceof THREE.Vector3 ||
//             this.camera instanceof THREE.OrthographicCamera ||
//             this._target.boundingBox == null) {
//           // TODO, with orthographic camera we could 'zoom in'
//           // by recalculating scene bounds but we have no handle
//           // to scene...
//           lookAtTarget();
//         } else if (target instanceof Object3D && thistarget.boundingBox != null) {
//           var pCamera = this.camera as PerspectiveCamera;
//           var bb = target.boundingBox;
//           var min = bb.min;
//           var max = bb.max;
//           var len = Math.abs(max.x - min.x);
//           var ylen = Math.abs(max.y - min.y);
//           if (ylen > len) len = ylen;
//           var zlen = Math.abs(max.z - min.z);
//           if (zlen > len) len = zlen;
//           var angle = (pCamera.fov / 360.0) * 2.0 * Math.PI;
//           var frameDist =
//               ((len / 2.0) / Math.sin(angle / 2.0)) * Math.cos(angle / 2.0);
//           zoomToTarget(frameDist, true);
//         }
//       }
//
//       /// Look at the current set target
//       ///
//       lookAtTarget() {
//         this.camera.lookAt(targetPosition());
//       }
//
//       /// Move closer to the target by the given amount
//       /// Positive zooms in,
//       /// Negative zooms out
//       /// zoomAmount is treated as relative to current position
//       /// unless absolute is true
//       zoomToTarget(zoomAmount:double, absolute:bool = false) {
//         if (!this.enabled) return;
//         var cameraTargetDelta = _targetPosition() - _camera.position;
//         var vecToTarget = cameraTargetDelta.normalized();
//         var distanceToTarget = cameraTargetDelta.length;
//         var newCamDistance = distanceToTarget - zoomAmount;
//         if (newCamDistance < minZoomDistance) {
//           zoomAmount = distanceToTarget - minZoomDistance;
//           // Never perfectly zero, else zoomout breaks
//           // because vector to target becomes zero length
//           if (minZoomDistance == 0) {
//             zoomAmount -= 0.001;
//           }
//         } else if (newCamDistance > maxZoomDistance) {
//           zoomAmount = distanceToTarget - maxZoomDistance;
//         }
//         _camera.position.add(vecToTarget * zoomAmount);
//         lookAtTarget();
//         window.console.log(vecToTarget);
//         window.console.log(zoomAmount);
//       }
//     //
//     //   rotateTheta(double theta) {
//     //     _startRotation();
//     //     _rotateCamera(theta, 0.0);
//     //   }
//     //
//     //   rotatePhi(double phi) {
//     //     _startRotation();
//     //     _rotateCamera(0.0, phi);
//     //   }
//     //
//     //   //--------------------------------------------------------------------
//     //   // Handlers
//     //   //--------------------------------------------------------------------
//     //
//     //   _hookHandlers() {
//     //     _handlerSubscriptions.add(
//     //         _sceneDomElement.onMouseEnter.listen(_handleSceneDomElementMouseEnter));
//     //     _handlerSubscriptions.add(
//     //         _sceneDomElement.onMouseLeave.listen(_handleSceneDomElementMouseLeave));
//     // //    _handlerSubscriptions.add(window.onKeyDown.listen(handleKeyboardEventDown));
//     // //    _handlerSubscriptions.add(window.onKeyUp.listen(handleKeyboardEventUp));
//     //     _handlerSubscriptions
//     //         .add(window.onMouseMove.listen(_handleWindowMouseMove));
//     //     _handlerSubscriptions
//     //         .add(window.onMouseDown.listen(_handleWindowMouseDown));
//     //     _handlerSubscriptions.add(window.onMouseUp.listen(_handleWindowMouseUp));
//     //     _handlerSubscriptions
//     //         .add(window.onMouseWheel.listen(_handleWindowMouseScroll));
//     //   }
//     //
//     //   _unhookHandlers() {
//     //     _handlerSubscriptions.forEach((s) => s.cancel());
//     //   }
//     //
//     //   _handleWindowMouseDown(MouseEvent e) {
//     //     if (e.button == 0 && _inSceneDomElement) {
//     //       e.preventDefault();
//     //       _active = true;
//     //       _start.setValues(e.screen.x + 0.0, e.screen.y + 0.0);
//     //       _screenWidth = window.screen.width;
//     //       _screenHeight = window.screen.height;
//     //       _startRotation();
//     //       // Using mathematical spherical coordinates
//     //       //print('Start: ${_startR} ${_startTheta} ${_startPhi}');
//     //     }
//     //   }
//     //
//     //   _startRotation() {
//     //     var camTargetDelta = (_camera.position - _targetPosition());
//     //     _startR = camTargetDelta.length;
//     //     camTargetDelta.normalize();
//     //     _startTheta = Math.atan2(camTargetDelta.y, camTargetDelta.x);
//     //     _startPhi = Math.acos(camTargetDelta.z / camTargetDelta.length);
//     //   }
//     //
//     //   _handleSceneDomElementMouseEnter(MouseEvent e) {
//     //     _inSceneDomElement = true;
//     //   }
//     //
//     //   _handleSceneDomElementMouseLeave(MouseEvent e) {
//     //     _inSceneDomElement = false;
//     //   }
//     //
//     //   _handleWindowMouseMove(MouseEvent e) {
//     //     if (_active && (_inSceneDomElement || useWholeWindow)) {
//     //       var pos = new Vector2(e.screen.x + 0.0, e.screen.y + 0.0);
//     //       var distanceX = -(pos.x - _start.x);
//     //       var distanceY = -(pos.y - _start.y);
//     //       var deltaTheta = (distanceX / _screenWidth) * 2.0 * Math.PI;
//     //       // Phi only varies over 180 degrees or 1 pi radians
//     //       var deltaPhi = (distanceY / _screenHeight) * Math.PI;
//     //       _rotateCamera(deltaTheta, deltaPhi);
//     //     }
//     //   }
//     //
//     //   _handleWindowMouseScroll(WheelEvent e) {
//     //     if (!_inSceneDomElement) return;
//     //     if (e.deltaY > 0) {
//     //       zoomToTarget(-10.0);
//     //     } else if (e.deltaY < 0) {
//     //       zoomToTarget(10.0);
//     //     }
//     //   }
//     //
//     //   _rotateCamera(deltaTheta, deltaPhi) {
//     //     var theta = _startTheta + deltaTheta;
//     //     // Phi only varies over 180 degrees or 1 pi radians
//     //     var phi = _startPhi + deltaPhi;
//     //     if (phi < phiMin) {
//     //       phi = phiMin;
//     //     } else if (phi > phiMax) {
//     //       phi = phiMax;
//     //     }
//     //     if (phi <= 0) phi = _min_phi_delta;
//     //     if (phi >= Math.PI) phi = Math.PI - _min_phi_delta;
//     //     var x = _startR * Math.sin(phi) * Math.cos(theta);
//     //     var y = _startR * Math.sin(phi) * Math.sin(theta);
//     //     var z = _startR * Math.cos(phi);
//     //     var tp = _targetPosition();
//     //     _camera.position.setValues(x + tp.x, y + tp.y, z + tp.z);
//     //     lookAtTarget();
//     //   }
//     //
//     //   _handleWindowMouseUp(MouseEvent e) {
//     //     if (e.button == 0) {
//     //       _active = false;
//     //     }
//     //   }
//     //
//     //   _targetPosition() {
//     //     if (_target is Vector3) {
//     //       return _target;
//     //     }
//     //     if (_target is Object3D) {
//     //       return _target.position;
//     //     }
//     //     return new Vector3.zero();
//     //   }
//     //
//     //   handleKeyboardEventDown(KeyboardEvent kbe) {
//     //     KeyEvent ke = new KeyEvent.wrap(kbe);
//     //     // ke.repeat currently stupidly unimplemented...
//     //     if (ke.shiftKey &&
//     //         (ke.keyCode == KeyCode.UP || ke.keyCode == KeyCode.DOWN)) {
//     //       if (!kbe.repeat && _zoomActiveKeyCode == null) {
//     //         //print("Zoom START");
//     //         _zoomActiveKeyCode = ke.keyCode;
//     //         _zoomStartTime = kbe.timeStamp;
//     //       }
//     //       var sign = 1.0;
//     //       if (ke.keyCode == KeyCode.DOWN) {
//     //         sign = -1.0;
//     //       }
//     //       var t = (kbe.timeStamp - _zoomStartTime) / 1000.0 + 0.25;
//     //       _currZoomSpeed = _currZoomSpeed + zoomAccelPerS * t;
//     //       if (_currZoomSpeed > maxZoomSpeed) _currZoomSpeed = maxZoomSpeed;
//     //       var zoomDistance = sign * _currZoomSpeed * t;
//     //       var zoomDelta = zoomDistance - _zoomTotalDistance;
//     //       _zoomTotalDistance = zoomDistance;
//     //       //print("${kbe.repeat} ${t}: zooming ${zoomDelta} total ${_zoomTotalDistance}");
//     //       zoomToTarget(zoomDelta);
//     //     }
//     //   }
//     //
//     //   handleKeyboardEventUp(KeyboardEvent kbe) {
//     //     KeyEvent ke = new KeyEvent.wrap(kbe);
//     //     //window.console.log(kbe);
//     //     if (ke.shiftKey && (ke.keyCode == _zoomActiveKeyCode)) {
//     //       //print("Zoom Stop");
//     //       _zoomActiveKeyCode = null;
//     //       _currZoomSpeed = 0.0;
//     //       _zoomStartTime = 0;
//     //       _zoomTotalDistance = 0.0;
//     //     }
//     //   }
//
//       get target(): THREE.Vector3 | THREE.Object3D {
//         return this._target;
//       }
//
//       void set target(dynamic newTarget) {
//         this._target = newTarget;
//         lookAtTarget();
//       }
//
//       get enabled():boolean {
//         return this._enabled;
//       }
//
//       set enabled(val:boolean) {
//         this._enabled = val;
//         if (this._enabled) {
//           _hookHandlers();
//         } else {
//           _unhookHandlers();
//         }
//       }
//   }
//
//
// }
