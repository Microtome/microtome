// import THREE = require('three');

@component("scene-holder")
class SceneHolder extends polymer.Base {

  @property({ readOnly: true})
  scene: THREE.Scene = new THREE.Scene();

  ready() {
    console.log(this['is'], "ready!")
  }

}

SceneHolder.register();
