/**
* Main app controller class that manages the behaviors of all sub components
*
* Supports slicing, etc.
*
*/
@component('microtome-app')
class MicrotomeApp extends polymer.Base {

  @property({ readOnly: true, value: () => new microtome.three_d.PrinterScene })
  public scene: microtome.three_d.PrinterScene;

  @property({ readOnly: false, notify: true })
  public printerConfig: microtome.printer.PrinterConfig = {
    name: 'unknown',
    description: 'none',
    lastModified: null,
    volume: {
      width: 120,
      depth: 120,
      height: 120
    }
  };

  // @property({ notify: true, value: () => false })
  // public hidePvView: boolean;

  @property({ notify: true })
  public hideSlicePreview: boolean = true;

  public toggleSlicePreview(e: Event) {
    this.hideSlicePreview = !this.hideSlicePreview
    if (this.hideSlicePreview) {
      this.$['main-pages'].selected = 0;
    } else {
      this.$['main-pages'].selected = 1;
    }
  }

  ready() {
    var sphere = new THREE.SphereGeometry(10);
    var mesh = new THREE.Mesh(sphere, microtome.three_d.CoreMaterialsFactory.objectMaterial);
    mesh.position.z = 10;
    this.scene.printObjects.push(mesh);
    console.log(this['is'], 'ready!')
  }

  attached() {
    window.console.log(this.$['sa-pv'])
    window.console.log(this.$['sa-sp'])
    this.$['sa-pv'].sharedElements = { 'hero': this.$['slice-preview-button'] }
    this.$['sa-pv'].animationConfig = {
      'entry': [
        {
          name: 'fade-in-animation',
          node: this.$['sa-pv'],
        },
        {
          name: 'hero-animation',
          id: 'hero',
          toPage: this.$['sa-pv']
        }
      ],
      'exit': [
        {
          name: 'hero-animation',
          id: 'hero',
          fromPage: this.$['sa-pv']
        },
        {
          name: 'fade-out-animation',
          node: this.$['sa-pv'],
        }
      ]
    }

    this.$['sa-sp'].sharedElements = { 'hero': this.$['slice-preview'] }
    this.$['sa-sp'].animationConfig = {
      'entry': [
        {
          name: 'hero-animation',
          id: 'hero',
          toPage: this.$['sa-sp']
        },
        {
          name: 'fade-in-animation',
          node: this.$['sa-sp'],
        }
      ],
      'exit': [
        {
          name: 'hero-animation',
          id: 'hero',
          fromPage: this.$['sa-sp']
        },
        {
          name: 'fade-out-animation',
          node: this.$['sa-sp'],
        }
      ]
    }
  }

}

MicrotomeApp.register();
