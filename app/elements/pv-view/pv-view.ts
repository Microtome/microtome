
@component("pv-view")
class PrinterVolumeView extends polymer.Base {

  @property({notify: true})
  scene: THREE.Scene = null;

  sceneChanged(newValue: THREE.Scene, oldValue: THREE.Scene){
    console.log("CHANGE!");
    console.log(oldValue);
    console.log(newValue);
  }

  ready() {
    console.log(this['is'], "ready!")
  }

}

PrinterVolumeView.register();
