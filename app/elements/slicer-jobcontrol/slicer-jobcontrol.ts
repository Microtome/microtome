import ToZipSlicerJob = microtome.slicer_job.HeadlessToZipSlicerJob;
import PrinterConfig = microtome.printer.PrinterConfig;
import PrinterJobConfig = microtome.printer.PrintJobConfig;
import PrinterScene = microtome.three_d.PrinterScene;

@component("slicer-jobcontrol")
class SlicerJobControl extends polymer.Base {

  @property({ notify: false, readOnly: false, type: "number" })
  public progress: number;

  @property({ notify: false, readOnly: true, type: "object" })
  public printerCfg: PrinterConfig;

  @property({ notify: false, readOnly: true, type: "object" })
  public printJobCfg: PrinterJobConfig;

  @property({ notify: false, readOnly: true, type: "object" })
  public scene: PrinterScene;

  @property({ notify: false, readOnly: false, type: "string"})
  public progressMessage:string;

  private zipSlicer:ToZipSlicerJob =null;

  public startSliceJob(){
    this.zipSlicer = new ToZipSlicerJob(this.scene,this.printerCfg, this.printJobCfg);
    this.zipSlicer.execute();
  }

  public stopSliceJob(){
    this.zipSlicer.cancel();
  }

}

SlicerJobControl.register();
