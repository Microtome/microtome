/**
* Main app controller class that manages the behaviors of all sub components
*
* Supports slicing, etc.
*
*/
@component('microtome-app')
class MicrotomeApp extends polymer.Base {

  // Convenience imports
  _TPI = microtome.printer.ThreadUnits.TPI;
  _PITCH_MM = microtome.printer.ThreadUnits.PITCH_MM;
  _PITCH_IN = microtome.printer.ThreadUnits.PITCH_IN;
  _INCH = microtome.units.LengthUnit.INCH;
  _MM = microtome.units.LengthUnit.MILLIMETER;
  _CM = microtome.units.LengthUnit.CENTIMETER;
  private _convertLengthUnit = microtome.units.convertLengthUnit

  @property({ readOnly: false, notify: true, type: Object, value: () => new microtome.three_d.PrinterScene })
  public scene: microtome.three_d.PrinterScene;

  @property({ readOnly: false, notify: true, type: Object })
  public printerConfig: microtome.printer.PrinterConfig = {
    name: 'unknown',
    description: 'none',
    lastModified: null,
    volume: {
      width: 36,
      depth: 24,
      height: 50
    },
    zStage: {
      threadMeasure: 20,
      threadUnits: microtome.printer.ThreadUnits.TPI,
      stepsPerRev: 1024,
      microsteps: 1
    },
    projector: {
      xRes: 360,
      yRes: 240
    }
  };

  @property({ notify: true })
  public hideSlicePreview: boolean = true;

  @property({ readOnly: false, notify: false })
  public sliceAt: number = 0;

  @property({ readOnly: false, notify: true })
  public layerThickness: number;

  public toggleSlicePreview(e: Event) {
    this.hideSlicePreview = !this.hideSlicePreview
    if (this.hideSlicePreview) {
      this.$['main-pages'].selected = 0;
    } else {
      this.$['main-pages'].selected = 1;
    }
  }

  public ready() {
    var geom = new THREE.BoxGeometry(10,10,10);
    // var geom = new THREE.SphereGeometry(10);
    var mesh = new THREE.Mesh(geom, microtome.three_d.CoreMaterialsFactory.objectMaterial);
    mesh.position.z = 6;
    this.scene.printObjects.push(mesh);
    console.log(this['is'], 'ready!')
  }

  public attached() {
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

  @observe("printerConfig.volume.width,printerConfig.volume.depth,printerConfig.volume.height")
  printVolumeChanged(newWidth: number, newDepth: number, newHeight: number) {
    this.scene.printVolume.resize(newWidth, newDepth, newHeight);
  }

  @observe("printerConfig.zStage.threadMeasure,printerConfig.zStage.threadUnits,printerConfig.zStage.stepsPerRev,printerConfig.zStage.microsteps")
  zstageParamsChanged(newThreadMeasure: number, newThreadUnits: microtome.printer.ThreadUnits, newStepsPerRev: number, newMicrosteps: number) {
    if (newThreadUnits == this._TPI) {
      this.layerThickness = (this._convertLengthUnit(1 / newThreadMeasure / (newMicrosteps * newStepsPerRev), this._INCH, this._MM)) ;
    } else if (newThreadUnits == this._PITCH_IN) {
      this.layerThickness = this._convertLengthUnit(newThreadMeasure / (newMicrosteps * newStepsPerRev), this._INCH, this._MM);
    } else if (newThreadUnits == this._PITCH_MM) {
      this.layerThickness = newThreadMeasure / (newMicrosteps * newStepsPerRev);
    }
    window.console.log(this.layerThickness);
  }

  public sliceUp() {
    this.sliceAt += this.layerThickness
    if (this.sliceAt > this.scene.printVolume.height) this.sliceAt = this.scene.printVolume.height
    window.console.log(this.sliceAt);
  }

  public sliceDown() {
    this.sliceAt -= this.layerThickness
    if (this.sliceAt < 0) this.sliceAt = 0;
    window.console.log(this.sliceAt);
  }

  public sliceStart() {
    this.sliceAt = 0;
    window.console.log(this.sliceAt);
  }

  public sliceEnd() {
    this.sliceAt = this.scene.printVolume.height
    window.console.log(this.sliceAt);
  }
}

MicrotomeApp.register();
