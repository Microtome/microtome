import * as threeObj from "three";

declare global {
  // Black magic to bridge the world of the living
  // and the dead
  var THREE: typeof threeObj;
  interface Window {
    THREE: typeof threeObj
  }
}

declare module "@three/*" {

}
