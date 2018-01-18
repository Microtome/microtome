import * as printer from "./config";
import * as THREE from "three";
import * as mats from "./materials";

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
    var planeMaterial = mats.whiteMaterial.clone();
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
      mats.xLineMaterial);
    this.add(xLines1);
    var xLines2 = new THREE.LineSegments(xlineGeometry.clone(),
      mats.xLineMaterial);
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
      mats.yLineMaterial);
    this.add(yLines1);
    var yLines2 = new THREE.LineSegments(ylineGeometry.clone(),
      mats.yLineMaterial);
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
      mats.zLineMaterial);
    this.add(zLines1);
    var zLines2 = new THREE.LineSegments(zlineGeometry.clone(),
      mats.zLineMaterial);
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
