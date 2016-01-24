module microtome.printer {

  // export enum GpioPinUse {
  //
  // }

  /**
  * For printers driven directly via GPIO, descrbes what each pin does
  */
  export interface GpioProtocol {

  };

  /**
  * Configuration for GCode driven printers
  */
  export interface GcodeProtocol {
  };

  /**
  * Unit of measure for threads
  */
  export enum ThreadUnits {
    /** Thread pitch mm */
    LEAD_MM,
    /** Thread pitch inches */
    LEAD_IN
  };

  export interface ZStage {
    threadMeasure: number,
    threadUnits: ThreadUnits,
    stepsPerRev: number,
    microsteps: number
  };

  export interface Projector {
    xRes: number,
    yRes: number
  };

  /**
  * Specifies printer volume
  */
  export interface PrintVolume {
    width: number,
    depth: number,
    height: number
  };

  /**
  * Printer configuration class
  */
  export interface PrinterConfig {
    /** name of printer */
    name: string,
    /** description of printer */
    description: string,
    /** ms since epoch */
    lastModified: number,
    /**
    * Print volume
    * x = width
    * y = depth
    * z = height
    */
    volume: PrintVolume,

    zStage: ZStage,

    projector: Projector
    // protocol: GpioProtocol | GcodeProtocol
  };

  /**
  * Print job settings
  */
  export interface PrintJobConfig {
    /** Name of settings */
    name: string;
    /** Description of job*/
    decription: string;
    /** Selected layer thickness, µm */
    layerThickness: number;
    /** Settle time, ms */
    settleTime: number;
    /** Exposure time per layer, ms */
    layerExposureTime: number;
    /** Blank time, ms */
    blankTime: number;
    /** Retract distance, distance platforms move to peel print, mm */
    retractDistance: number;
    /**
    * Z offset for added objects. This is the amount objects are offset
    * when they are added to the scene
    */
    zOffset: number;
    /**
    * Thickness of printed raft, used to 'stick' items to build platform
    * raftThickness <= zOffset;
    */
    raftThickness: number;
  };

}
