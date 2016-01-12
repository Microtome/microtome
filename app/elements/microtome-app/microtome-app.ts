
@component("microtome-app")
class MicrotomeApp extends polymer.Base {

  @property({ readOnly: true, value: () => new THREE.Scene() })
  public scene: THREE.Scene;

  // @property({ notify: true, value: () => false })
  // public hidePvView: boolean;

  @property({ notify: true, value: () => true })
  public hideSlicePreview: boolean;

  onMenuActivate(event: Event, detail: Object) {
    var menu = Polymer.dom(event).localTarget;
    menu.select(-1);
  }

  ready() {
    console.log(this['is'], "ready!")
  }

}

MicrotomeApp.register();
