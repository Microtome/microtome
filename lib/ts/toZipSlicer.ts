module microtome.slicer {
  export class ToZipSlicer {

    constructor(slicer:AdvancedSlicer){
    }

    do(cfg: microtome.printer.PrinterConfig):JSZip{
      let zip = new JSZip();
      return zip;
    }
  }
}
