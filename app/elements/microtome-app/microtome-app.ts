/**
* Main app controller class that manages the behaviors of all sub components
*
* Supports slicing, etc.
*
*/
@component("microtome-app")
class MicrotomeApp extends polymer.Base {

  @property({ readOnly: true, value: () => new microtome.three_d.PrinterScene })
  public scene: microtome.three_d.PrinterScene;

  @property({ readOnly: false, notify: true })
  public printerConfig: microtome.printer.PrinterConfig = {
    name: "unknown",
    description: "none",
    lastModified: null,
    volume: {
      width: 120,
      depth: 120,
      height: 120
    }
  };

  // @property({ notify: true, value: () => false })
  // public hidePvView: boolean;

  @property({ notify: true, value: () => true })
  public hideSlicePreview: boolean;

  public toggleSlicePreview() {
    this.hideSlicePreview = !this.hideSlicePreview;
  }

  ready() {
    var sphere = new THREE.SphereGeometry(10);
    var mesh = new THREE.Mesh(sphere, microtome.three_d.CoreMaterialsFactory.objectMaterial);
    mesh.position.z = 10;
    this.scene.add(mesh);
    // this.scene.add(new PrinterVolumeView(120,12,120))
    console.log(this['is'], "ready!")
  }

}

MicrotomeApp.register();
