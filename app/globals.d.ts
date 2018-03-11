import * as threeObj from "three";

declare global {
  // Black magic to bridge the world of the living
  // and the dead
  var THREE: typeof threeObj;
  interface Window {
    THREE: typeof threeObj
  }

  namespace JSX {
    // Needed so that Surplus typechecks as it produces real dom nodes
    // not fake ones.
    interface Element extends HTMLElement{

    }
  }
}

declare module "@three/*" {

}
