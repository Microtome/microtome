module microtome.three_d {

  /**
    * Properly encoding 32 bit float in rgba from here:
    * http://www.gamedev.net/topic/486847-encoding-16-and-32-bit-floating-point-value-into-rgba-byte-texture/
    */
  export class CoreMaterialsFactory {
    static _basicVertex = `
void main(void) {
   // compute position
   gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

    static _depthShaderFrag = `

    vec4 pack( const in float depth ) {
        const float toFixed = 255.0/256.0;
        vec4 result = vec4(0);
        result.r = fract(depth*toFixed*1.0);
        result.g = fract(depth*toFixed*255.0);
        result.b = fract(depth*toFixed*255.0*255.0);
        result.a = fract(depth*toFixed*255.0*255.0*255.0);
        return result;
    }

    void main() {
      gl_FragColor = pack(gl_FragCoord.z);
    }`;

    static _sliceShaderFrag = `
// current slice height in device coordinates
uniform float cutoff;

// Texture containing depth info of scene
// packed into rgba
// Must be same size as current viewport
uniform sampler2D dTex;

// Since depth conversion not exact
// Use epsilon for compare
// 1/2 slice height should be good.
uniform float epsilon;

uniform int viewWidth;
uniform int viewHeight;

float unpack(const in vec4 color) {
  const float fromFixed = 256.0/255.0;
  float result = color.r*fromFixed/(1.0)
  +color.g*fromFixed/(255.0)
  +color.b*fromFixed/(255.*255.0)
  +color.a*fromFixed/(255.0*255.0*255.0);
  return result;
}

bool isInLattice(const in vec4 fragCoord, const in float cutOff){
  float modX = abs(mod(fragCoord.x, 20.0));
  float modY = abs(mod(fragCoord.y, 20.0));
  float modZ = abs(mod(cutOff * 1000.0, 20.0));

  return ( (modX <= 1.0 && modY <= 1.0) || (modX <= 1.0 && modZ <= 10.0) || (modY <= 1.0 && modZ <= 10.0)  );
}

void main(void) {
  vec2 lookup = gl_FragCoord.xy / vec2(viewWidth, viewHeight );
  float depth = unpack(texture2D(dTex, lookup));

  float zCutoff = 1.0 - cutoff;

  if ( gl_FragCoord.z < zCutoff ){
    discard;
  }

  if(gl_FrontFacing){
    float modX = abs(mod(gl_FragCoord.x, 20.0));
    float modY = abs(mod(gl_FragCoord.y, 20.0));
    float modZ = abs(mod(gl_FragCoord.z * 20.0 , 20.0));
    gl_FragColor = vec4(vec3(0), 1);
    if( isInLattice(gl_FragCoord, zCutoff)
      && gl_FragCoord.z > epsilon + depth && zCutoff > depth  ){
      gl_FragColor = vec4(vec3(1), 1);
    }
  } else {
    gl_FragColor = vec4(vec3(1),1);
  }
}`;

    static xLineMaterial = new THREE.LineBasicMaterial({ color: 0xd50000, linewidth: 2 });
    static yLineMaterial = new THREE.LineBasicMaterial({ color: 0x00c853, linewidth: 2 });
    static zLineMaterial = new THREE.LineBasicMaterial({ color: 0x2962ff, linewidth: 2 });
    static bBoxMaterial = new THREE.LineBasicMaterial({ color: 0x4fc3f7, linewidth: 2 });
    static whiteMaterial = new THREE.MeshLambertMaterial({ color: 0xf5f5f5, side: THREE.DoubleSide });
    static objectMaterial = new THREE.MeshPhongMaterial({ color: 0xCFCFCF, side: THREE.DoubleSide });//, ambient:0xcfcfcf});
    static selectMaterial = new THREE.MeshPhongMaterial({ color: 0x00CFCF, side: THREE.DoubleSide });//, ambient:0x00cfcf});
    /**
    Material for rendering depth textures
    */
    static depthMaterial = new THREE.ShaderMaterial({
      fragmentShader: CoreMaterialsFactory._depthShaderFrag, vertexShader: CoreMaterialsFactory._basicVertex, blending: THREE.NoBlending
    });
    /**
    Material for slicing
    */
    static sliceMaterial = new THREE.ShaderMaterial({
      fragmentShader: CoreMaterialsFactory._sliceShaderFrag,
      vertexShader: CoreMaterialsFactory._basicVertex,
      side: THREE.DoubleSide,
      blending: THREE.NoBlending,
      uniforms: { 'cutoff': 0.00 }
    });
  }


  type CameraTarget = THREE.Vector3 | THREE.Mesh;

  export class CameraNav {
    _sceneDomElement: HTMLElement;
    _camera: THREE.Camera;
    //   final Logger log = new Logger('camera_nav');
    //
    /// Camera nav enabled?
    _enabled: boolean;
    _target: CameraTarget = new THREE.Vector3(0.0, 0.0, 0.0);
    homePosition = new THREE.Vector3(0.0, 0.0, 100.0);
    /// Do we follow mouse movements across the whole window?
    useWholeWindow = true;
    /// Is zooming enabled?
    allowZoom = false;
    /// Minimum distance to zoom in to
    minZoomDistance = 0.0;
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
    _start = new THREE.Vector2(0.0, 0.0);

    // private zoomActiveKeyCode:KeyCo = null;
    _currZoomSpeed = 0.0;
    _zoomStartTime = 0;
    _zoomTotalDistance = 0.0;
    //
    //   List<StreamSubscription> _handlerSubscriptions = [];
    //
    /// If thetaMin == thetaMax,then free spinning around the z axis is assumed
    /// If useWholeWindow is true, then even mouse pointer leaves bounds of sceneDomElement
    /// camera will still rotate around scene. if set to false, camera navigation stops
    /// when pointer leaves scene element
    ///
    /// By default, camera target is Vector3(0.0, 0.0, 0.0) in global space
    constructor(camera: THREE.Camera, sceneDomElement: HTMLElement, enabled: boolean = true) {
      this._camera = camera;
      this._sceneDomElement = sceneDomElement;
      this.homePosition = camera.position.clone();
      this.enabled = enabled;
    }

    /// Send the camera to the home position and
    /// have it look at the target
    ///
    /// Home position may be set in constructor or using
    /// property of same name. Default value is Vector3 (0.0,0.0,100.0)
    ///
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
    //
    //   //--------------------------------------------------------------------
    //   // Handlers
    //   //--------------------------------------------------------------------
    //
    //   _hookHandlers() {
    //     _handlerSubscriptions.add(
    //         _sceneDomElement.onMouseEnter.listen(_handleSceneDomElementMouseEnter));
    //     _handlerSubscriptions.add(
    //         _sceneDomElement.onMouseLeave.listen(_handleSceneDomElementMouseLeave));
    // //    _handlerSubscriptions.add(window.onKeyDown.listen(handleKeyboardEventDown));
    // //    _handlerSubscriptions.add(window.onKeyUp.listen(handleKeyboardEventUp));
    //     _handlerSubscriptions
    //         .add(window.onMouseMove.listen(_handleWindowMouseMove));
    //     _handlerSubscriptions
    //         .add(window.onMouseDown.listen(_handleWindowMouseDown));
    //     _handlerSubscriptions.add(window.onMouseUp.listen(_handleWindowMouseUp));
    //     _handlerSubscriptions
    //         .add(window.onMouseWheel.listen(_handleWindowMouseScroll));
    //   }
    //
    //   _unhookHandlers() {
    //     _handlerSubscriptions.forEach((s) => s.cancel());
    //   }
    //
    // _handleWindowMouseDown(e:MouseEvent ) {
    //   if (e.button == 0 && this._inSceneDomElement) {
    //     e.preventDefault();
    //     this._active = true;
    //     this._start.set(e.screenX, e.screenY);
    //     this._screenWidth = window.screen.width;
    //     this._screenHeight = window.screen.height;
    //     this._startRotation();
    //     // Using mathematical spherical coordinates
    //     //print('Start: ${_startR} ${_startTheta} ${_startPhi}');
    //   }
    // }



    _startRotation() {
      var camTargetDelta = (this._camera.position.clone().sub(this._targetPosition()));
      this._startR = camTargetDelta.length();
      camTargetDelta.normalize();
      this._startTheta = Math.atan2(camTargetDelta.y, camTargetDelta.x);
      this._startPhi = Math.acos(camTargetDelta.z / this._startR);
    }

    _handleSceneDomElementMouseEnter(e: MouseEvent) {
      this._inSceneDomElement = true;
    }

    _handleSceneDomElementMouseLeave(e: MouseEvent) {
      this._inSceneDomElement = false;
    }

    _handleWindowMouseMove(e: MouseEvent) {
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

    _handleWindowMouseScroll(e: WheelEvent) {
      if (!this._inSceneDomElement) return;
      if (e.deltaY > 0) {
        this.zoomToTarget(-10.0);
      } else if (e.deltaY < 0) {
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

    _handleWindowMouseUp(e: MouseEvent) {
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

    // handleKeyboardEventDown( kbe:KeyboardEvent) {
    //
    //   // kbe.repeat currently stupidly unimplemented...
    //   if (kbe.shiftKey &&
    //     (kbe.keyCode == KeyCode.UP || kbe.keyCode == KeyCode.DOWN)) {
    //     if (!kbe.repeat && this._zoomActiveKeyCode == null) {
    //       //print('Zoom START');
    //       this._zoomActiveKeyCode = kbe.kbeyCode;
    //       this._zoomStartTime = kbe.timeStamp;
    //     }
    //     var sign = 1.0;
    //     if (kbe.keyCode == '') {
    //       sign = -1.0;
    //     }
    //     var t = (kbe.timeStamp - this._zoomStartTime) / 1000.0 + 0.25;
    //     this._currZoomSpeed = this._currZoomSpeed + zoomAccelPerS * t;
    //     if (this._currZoomSpeed > maxZoomSpeed) this._currZoomSpeed = maxZoomSpeed;
    //     var zoomDistance = sign * this._currZoomSpeed * t;
    //     var zoomDelta = zoomDistance - this._zoomTotalDistance;
    //     this._zoomTotalDistance = zoomDistance;
    //     //print('${kbe.repeat} ${t}: zooming ${zoomDelta} total ${this._zoomTotalDistance}');
    //     this.zoomToTarget(zoomDelta);
    //   }
    // }

    // handleKeyboardEventUp(kbe: KeyboardEvent) {
    //   //window.console.log(kbe);
    //   if (ke.shiftKey && (ke.keyCode == _zoomActiveKeyCode)) {
    //     //print('Zoom Stop');
    //     this._zoomActiveKeyCode = null;
    //     this._currZoomSpeed = 0.0;
    //     this._zoomStartTime = 0;
    //     this._zoomTotalDistance = 0.0;
    //   }
    // }

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
      // if (this._enabled) {
      //   this._hookHandlers();
      // } else {
      //   this._unhookHandlers();
      // }
    }
  }


  /// All dimensions are in mm
  export class PrintVolume extends THREE.Group {
    _width: number = 100.0;
    _depth: number = 100.0;
    _height: number = 100.0;

    constructor(width: number, depth: number, height: number) {
      super();
      this._width = width;
      this._height = height;
      this._depth = depth;
      var planeGeom: THREE.PlaneGeometry = new THREE.PlaneGeometry(1.0, 1.0);

      var planeMaterial = CoreMaterialsFactory.whiteMaterial.clone();
      planeMaterial.side = THREE.DoubleSide;

      var plane = new THREE.Mesh(planeGeom, planeMaterial);
      this.add(plane);

      var xlinesPts = [
        new THREE.Vector3(-0.5, 0.5, 0.0),
        new THREE.Vector3(0.5, 0.5, 0.0),
        new THREE.Vector3(-0.5, -0.5, 0.0),
        new THREE.Vector3(0.5, -0.5, 0.0)
      ];
      var xlineGeometry = new THREE.Geometry();
      xlineGeometry.vertices = xlinesPts;
      var xLines1 = new THREE.Line(xlineGeometry.clone(),
        CoreMaterialsFactory.xLineMaterial, THREE.LinePieces);
      this.add(xLines1);
      var xLines2 = new THREE.Line(xlineGeometry.clone(),
        CoreMaterialsFactory.xLineMaterial, THREE.LinePieces);
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
      var yLines1 = new THREE.Line(ylineGeometry.clone(),
        CoreMaterialsFactory.yLineMaterial, THREE.LinePieces);
      this.add(yLines1);
      var yLines2 = new THREE.Line(ylineGeometry.clone(),
        CoreMaterialsFactory.yLineMaterial, THREE.LinePieces);
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
      var zLines1 = new THREE.Line(zlineGeometry.clone(),
        CoreMaterialsFactory.zLineMaterial, THREE.LinePieces);
      this.add(zLines1);
      var zLines2 = new THREE.Line(zlineGeometry.clone(),
        CoreMaterialsFactory.zLineMaterial, THREE.LinePieces);
      zLines2.position.set(0.0, -1.0, 0.0);
      this.add(zLines2);
      this.scale.set(this._width, this._depth, this._height);
    }

    resize(width: number, depth: number, height: number) {
      this.scale.set(width, depth, height);
    }

    /// Hides the printer volume including all its component parts
    // Not needed anymore in r71, hiding parent hides children
    // set visible(visible: boolean) {
    //   this._visible = visible;
    //   this.children.forEach(function(child){ child.visible = visible});
    // }
    //
    // get visible():boolean {
    //   return this._visible;
    // }

    get boundingBox(): THREE.Box3 {
      /// TODO Cache better
      var halfWidth = this._width / 2.0;
      var halfDepth = this._depth / 2.0;
      var min = new THREE.Vector3(-halfWidth, -halfDepth, 0.0);
      var max = new THREE.Vector3(halfWidth, halfDepth, this._height);
      return new THREE.Box3(min, max);
    }
  }


}
