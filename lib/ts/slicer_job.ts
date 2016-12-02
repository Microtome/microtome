module microtome.slicer_job {

  export interface SliceJobConfig {
    layers: SliceJobLayerConfig,
    retractMM: number,
    raft: SliceJobRaftConfig,
    maxZ?: number
  }

  export interface SliceJobRaftConfig {
    thicknessMM: number,
    layers: SliceJobLayerConfig,
    dialateMM: number,
  }

  export interface SliceJobLayerConfig {
    thicknessMicrons: number,
    exposureTimeSecs: number
  }

  /**
  * This slicer is headless, slicing a given scene to a zip file
  * containing containing images and slice information
  *
  * TODO add ability to set preview dom element for slice images
  *
  */
  export class HeadlessToZipSlicer {

    /**
    *
    */
    static execute(scene: microtome.three_d.PrinterScene,
      cfg: microtome.printer.PrinterConfig,
      jobCfg: SliceJobConfig, validate: boolean = false): Promise<Blob> {
      return (new HeadlessToZipSlicerJob(scene, cfg, jobCfg)).execute(validate);
    }
  }

  /**
  * Class that actually handles the slicing job. Not reusable
  */
  class HeadlessToZipSlicerJob {

    private readonly slicer: microtome.slicer.AdvancedSlicer;
    private renderer: THREE.WebGLRenderer = new THREE.WebGLRenderer({ alpha: false, antialias: false, clearColor: 0x000000 });
    private canvasElement: HTMLCanvasElement = this.renderer.domElement;
    private raftThickness: number = 0;
    private raftZStep: number = 0;
    private zStep: number = 0;

    private z = 0;
    private sliceNum = 0;
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

    constructor(private scene: microtome.three_d.PrinterScene,
      private cfg: microtome.printer.PrinterConfig,
      private jobCfg: SliceJobConfig) {
      let shellInset = -1;
      let raftOffset = jobCfg.raft.dialateMM || 0;
      let pixelWidthMM = this.scene.printVolume.width / cfg.projector.xRes;
      let pixelHeightMM = this.scene.printVolume.depth / cfg.projector.yRes;
      this.raftThickness = this.jobCfg.raft.thicknessMM;
      this.raftZStep = this.jobCfg.raft.layers.thicknessMicrons / 1000;
      this.zStep = this.jobCfg.layers.thicknessMicrons / 1000;
      this.renderer.setSize(cfg.projector.xRes, cfg.projector.xRes);
      this.canvasElement.style.width = `${cfg.projector.xRes}px`;
      this.canvasElement.style.height = `${cfg.projector.xRes}px`;
      this.canvasElement.width = cfg.projector.xRes;
      this.canvasElement.height = cfg.projector.yRes;
      this.slicer = new microtome.slicer.AdvancedSlicer(scene,
        pixelWidthMM,
        pixelHeightMM,
        this.raftThickness,
        raftOffset,
        shellInset,
        this.renderer)
    }

    private doSlice() {
      if (this.z < this.raftThickness) {
        this.z = this.z + this.raftZStep;
        // raft slice, handled by slicer, but we
        // we need to handle z steps
      } else {
        this.z = this.z + this.zStep;
        // regular slice
      }
      // let data = this.slicer.sliceAtToImageBase64(this.z);
      // let base64Data = data.slice(data.indexOf(",") + 1);
      // this.zip.file(`${this.sliceNum}.png`, base64Data, { base64: true, compression: "store" })
      // this.sliceNum++;
      // this.scheduleNextSlice();
      // if (this.sliceNum % 20 == 0) {
      //   console.log(`Layer ${this.sliceNum}, height: ${this.z}`);
      // }
      this.slicer.sliceAtToBlob(this.z, blob => {
        // console.log("SLICE!!!");
        this.zip.file(`${this.sliceNum}.png`, blob, {  compression: "store" })
        this.sliceNum++;
        if (this.sliceNum % 20 == 0) {
          console.log(`Layer ${this.sliceNum}, height: ${this.z}`);
        }
        this.scheduleNextSlice();
      });
      ;
      // return this.zip.generateAsync({ type: "blob" });
      // TODO Need to generate zip after adding all files. Not quite right still
    }

    private scheduleNextSlice() {
      if (this.z <= this.scene.printVolume.height) {
        this.handle = setTimeout(this.doSlice.bind(this), this.SLICE_TIME);
      } else {
        if (this.handle) {
          clearTimeout(this.handle);
        }
        this.zip.generateAsync({ type: "blob" }).then(blob => this.resolve(blob));
      }
    }

    execute(validate: boolean = false): Promise<Blob> {
      let config = {
        job: this.jobCfg,
        printer: this.cfg
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
}
