
@component("layer-height")
class LayerHeight extends polymer.Base {

  public INCH = microtome.units.LengthUnit.INCH
  public MM = microtome.units.LengthUnit.MILLIMETER
  public convertLengthUnit = microtome.units.convertLengthUnit

  /**
  * Layer Height in mm
  */
  @property({ notify: true, readOnly: false, type: 'number' })
  public layerHeight: number = 0.0;

  @property({ notify: true, readOnly: false, type: "string" })
  public threadLabel: string;

  @property({ notify: true, readOnly: false, type: "boolean" })
  public manualOverride: boolean;

  @property({ notify: true, readOnly: false, type: "number" })
  public threadStep: number;

  @property({ notify: true, readOnly: false, type: "number" })
  public microsteps: number;

  @property({ notify: true, readOnly: false, type: "number" })
  public steps: number;

  private threadUnitGroup: PaperRadioGroup;

  private threadMeasure: PaperInput;

  attached() {
    this.threadUnitGroup = this.$["thread-unit"] as PaperRadioGroup
    this.threadMeasure = this.$["thread-measure"] as PaperInput
    // TODO Move these to default value functions on property
    this.threadLabel = "tpi"
    this.manualOverride = false;
    this.microsteps = 1;
    this.steps = 1024;
    this.threadStep = 20;
  }

  @observe("manualOverride")
  manualOverrideChanged(newValue: Boolean, oldValue: Boolean) {
    if (!newValue) this.recalcLayerHeight(this.steps, this.microsteps, this.threadStep, this.threadLabel);
  }

  @observe("steps, microsteps,threadStep,threadLabel")
  recalcLayerHeight(newSteps: number, newMicrosteps: number, newThreadStep: number, newThreadLabel: string) {
    if (this.manualOverride) return;
    if (this.threadLabel && newThreadStep > 0 && this.microsteps > 0)
      if (this.threadLabel.indexOf("tpi") > -1) {
        this.layerHeight = (this.convertLengthUnit(1, this.INCH, this.MM) / newThreadStep) / (this.microsteps * this.steps);
      } else if (this.threadLabel.indexOf("in") > -1) {
        this.layerHeight = this.convertLengthUnit(newThreadStep / (this.microsteps * this.steps), this.INCH, this.MM);
      } else if (this.threadLabel.indexOf("mm") > -1) {
        this.layerHeight = newThreadStep / (this.microsteps * this.steps);
      }
  }

}

LayerHeight.register();
