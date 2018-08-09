/**
 * This module contains classes for managing asynchronous model slicing jobs.
 */

import * as config from "./config";
import * as printer from "./printer";
import * as slicer from "./slicer";

import "jszip";
import * as THREE from "three";

/**
 * Class that actually handles the slicing job. Not reusable
 */
export class HeadlessToZipSlicerJob {

  private readonly slicer: slicer.AdvancedSlicer;
  private raftThicknessMM: number = 0;
  private zStepMM: number = 0;

  private z = 0;
  private sliceNum = 1;
  private startTime = Date.now();
  private zip = new JSZip();
  private resolve: (blob: Blob) => void = null;
  private reject: (e?: Error) => void = null;
  private jobStartTime: Date = null;
  private zipBlob = new Promise<Blob>((resolve, reject) => {
    this.resolve = resolve;
    this.reject = reject;
  });
  // private readonly SLICE_TIME = 5;
  private cancelled = false;
  private maxSliceHeight = 0;

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
  constructor(private scene: printer.PrinterScene,
              private printerCfg: config.PrinterConfig,
              private jobCfg: config.PrintJobConfig) {
    const shellInsetMM = -1;
    const raftOutsetMM = jobCfg.raftOutset_mm || 0;
    const pixelWidthMM = this.scene.printVolume.width / printerCfg.projector.xRes_px;
    const pixelHeightMM = this.scene.printVolume.depth / printerCfg.projector.yRes_px;
    this.raftThicknessMM = this.jobCfg.raftThickness_mm;
    this.zStepMM = (this.jobCfg.stepDistance_microns * this.jobCfg.stepsPerLayer) / 1000;
    this.slicer = new slicer.AdvancedSlicer(scene,
      pixelWidthMM,
      pixelHeightMM,
      this.raftThicknessMM,
      raftOutsetMM,
      shellInsetMM);
    this.slicer.setSize(printerCfg.projector.xRes_px, printerCfg.projector.yRes_px);
    // TODO Remove once more intelligent print volume methods are added
    this.maxSliceHeight = this.scene.printObjects
      .map((mesh) => {
        mesh.geometry.computeBoundingBox();
        return mesh.position.z + mesh.geometry.boundingBox.max.z;
      })
      .reduce((prev, curr) => {
        return Math.max(prev, curr);
      }, 0) + this.zStepMM;
  }

  /**
   * Cancel the slicing job. Will cause the promise returned by
   * execute to fail
   */
  public cancel() {
    this.cancelled = true;
  }

  /**
   * Execute the slicing job
   *
   * @returns a Promise yiedling a zip compressed blob of slice images
   */
  public execute(validate: boolean = false): Promise<Blob> {
    this.startTime = Date.now();
    try {
      this.doSlice();
    } catch (e) {
      this.reject(e);
    }
    return this.zipBlob;
    // // Store config
    // this.zip.file("config.json", JSON.stringify(config))
  }

  private doSlice() {
    // TODO Error accumulation
    this.z = this.zStepMM * this.sliceNum;
    // console.debug(`Slicing ${this.sliceNum} at ${this.z}mm`)
    this.slicer.sliceAtToBlob(this.z, (blob) => {
      const sname = this.sliceNum.toString().padStart(8, "0");
      this.zip.file(`${sname}.png`, blob, { compression: "store" });
      this.sliceNum++;
      this.scheduleNextSlice();
    });
  }

  private scheduleNextSlice() {
    if (this.z <= this.maxSliceHeight && !this.cancelled) {
      this.doSlice();
    } else {
      if (this.cancelled) {
        this.reject();
      } else {
        const cfgObj = JSON.stringify({
          job: this.jobCfg,
          printer: this.printerCfg,
        }, null, 2);
        this.zip.file(`slice-config.json`, cfgObj);
        const slicingFinished = Date.now();
        this.zip.generateAsync({ type: "blob" }).then((blob) => {
          const zipEnd = Date.now();
          const sliceTime = ((slicingFinished - this.startTime) / 1000);
          const zipFinishedTime = ((zipEnd - slicingFinished) / 1000);
          const totalTime = sliceTime + zipFinishedTime;
          // console.debug(`Slicing Job Complete!`);
          // console.debug(`  Sliced ${this.sliceNum + 1} layers`);
          // console.debug(`  Slicing took ${sliceTime.toFixed(2)}s,
          // ${(sliceTime * 1000 / (this.sliceNum + 1)).toFixed(2)}ms / layer`);
          // console.debug(`  Zip generation took ${zipFinishedTime.toFixed(2)}s`);
          // console.debug(`  Total time took ${totalTime.toFixed(2)}s,
          // amortized ${(totalTime * 1000 / (this.sliceNum + 1)).toFixed(2)}ms / layer`);
          this.resolve(blob);
        });

      }
    }
  }
}
