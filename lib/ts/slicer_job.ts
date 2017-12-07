import * as slicer from "./advanced_slicer";
import * as printer from "./printer_config";
import * as three_d from "./core_threed";

import * as THREE from "three";
import * as JSZip from "jszip";

/**
* Class that actually handles the slicing job. Not reusable
*/
export class HeadlessToZipSlicerJob {

  private readonly slicer: slicer.AdvancedSlicer;
  private raftThickness_mm: number = 0;
  // private raftZStep_mm: number = 0;
  private zStep_mm: number = 0;

  private z = 0;
  private sliceNum = 1;
  private startTime = Date.now();
  private zip = new JSZip();
  private handle: number = null;
  private resolve: Function = null;
  private reject: Function = null;
  private zipBlob: Promise<Blob> = new Promise((resolve, reject) => {
    this.resolve = resolve;
    this.reject = reject;
  })
  private readonly SLICE_TIME = 20;
  private cancelled = false;

  /**
   * Create a headless slicing job that slices to
   * a zip compressed blob
   * 
   * INSTANCES CAN NOT BE REUSED
   * 
   * @param scene to slice
   * @param printerCfg printer configuration 
   * @param jobCfg job configuration
   */
  constructor(private scene: three_d.PrinterScene,
    private printerCfg: printer.PrinterConfig,
    private jobCfg: printer.PrintJobConfig) {
    let shellInset_mm = -1;
    let raftOutset_mm = jobCfg.raftOutset_mm || 0;
    let pixelWidthMM = this.scene.printVolume.width / printerCfg.projector.xRes_px;
    let pixelHeightMM = this.scene.printVolume.depth / printerCfg.projector.yRes_px;
    this.raftThickness_mm = this.jobCfg.raftThickness_mm;
    this.zStep_mm = (this.jobCfg.stepDistance_microns * this.jobCfg.stepsPerLayer) / 1000;
    this.slicer.setSize(printerCfg.projector.xRes_px, printerCfg.projector.yRes_px);
    this.slicer = new slicer.AdvancedSlicer(scene,
      pixelWidthMM,
      pixelHeightMM,
      this.raftThickness_mm,
      raftOutset_mm,
      shellInset_mm)
  }

  private doSlice() {
    // TODO Error accumulation
    this.z = this.zStep_mm * this.sliceNum;
    this.slicer.sliceAtToBlob(this.z, blob => {
      // console.log("SLICE!!!");
      let sname = this.sliceNum.toString().padStart(8, "0");
      this.zip.file(`${sname}.png`, blob, { compression: "store" })
      this.sliceNum++;
      this.scheduleNextSlice();
    });
    ;
    // return this.zip.generateAsync({ type: "blob" });
    // TODO Need to generate zip after adding all files. Not quite right still
  }

  private scheduleNextSlice() {
    if (this.z <= this.scene.printVolume.height && !this.cancelled) {
      this.handle = setTimeout(this.doSlice.bind(this), this.SLICE_TIME);
    } else {
      if (this.handle) {
        clearTimeout(this.handle);
      }
      if (this.cancelled) {
        this.reject();
      } else {
        this.zip.generateAsync({ type: "blob" }).then(blob => this.resolve(blob));
      }
    }
  }

  /**
  * Cancel the slicing job. Will cause the promise returned by
  * execute to fail
  */
  cancel() {
    this.cancelled = true;
  }

  /**
  * Execute the slicing job
  *
  * @returns a Promise yiedling a zip compressed blob of slice images 
  */
  execute(validate: boolean = false): Promise<Blob> {
    let config = {
      job: this.jobCfg,
      printer: this.printerCfg
    }
    try {
      this.doSlice();
      // this.scheduleNextSlice();
    } catch (e) {
      this.reject(e);
    }
    return this.zipBlob;
    // // Store config
    // this.zip.file("config.json", JSON.stringify(config))
  }
}
