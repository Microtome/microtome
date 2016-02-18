// Required due to typescript bug 6595
// <reference path="../typings/threejs/three.d.ts" />

// import THREE from "../typings/threejs/three.d.ts";

// declare module THREE_ex {

declare module THREE {
  export class STLLoader extends THREE.Loader {
    constructor(manager?: LoadingManager);
    manager: LoadingManager;
    withCredentials: boolean;

    load(url: string, onLoad?: (geometry: Geometry) => void, onProgress?: (event: any) => void, onError?: (event: any) => void): void;

  }

  export class OBJLoader extends THREE.Loader {
    constructor(manager?: LoadingManager);
    manager: LoadingManager;
    withCredentials: boolean;

    load(url: string, onLoad?: (geometry: Geometry) => void, onProgress?: (event: any) => void, onError?: (event: any) => void): void;


  }
}
// }
