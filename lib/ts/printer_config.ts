module microtome.printer {

  // export enum GpioPinUse {
  //
  // }

  export interface spjsConfig {
    port: number;
    host: string;
  }

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

  export interface ZStage {
    // How far does it travel per revolution
    lead_mm: number,
    // Full steps per revolution
    stepsPerRev: number,
    // Microsteps per full step
    microsteps: number
  };

  // CUrrently assumes square pixels
  export interface Projector {
    // Pixels x direction
    xRes_px: number;
    // Pixels y direction;
    yRes_px: number;
  };

  export interface Resin {
    manufacturer: string,
    productName: string,
    productNumber: string,
    pricePerUnit: string,
    unitVolume_ml: string,
  }

  /**
  * Specifies printer volume
  */
  export interface PrintVolume {
    width_mm: number,
    depth_mm: number,
    height_mm: number
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
    */
    volume: PrintVolume,
    // Z Stage info
    zStage: ZStage,
    // Projector info
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
    description: string;
    /** The step distance used when this job was created */
    stepDistance_microns: number;
    /** The number of steps per layer */
    stepsPerLayer: number;
    /** Settle time, ms */
    settleTime_ms: number;
    /** Exposure time per layer, ms */
    layerExposureTime_ms: number;
    /** Blank time, ms */
    blankTime_ms: number;
    /** Retract distance, distance platforms move to peel print, mm */
    retractDistance_mm: number;
    /**
    * Z offset for added objects. This is the amount objects are offset
    * when they are added to the scene
    */
    zOffset_mm: number;
    /**
    * Thickness of printed raft, used to 'stick' items to build platform
    * raftThickness <= zOffset;
    */
    raftThickness_mm: number;
    /**
    * How much distance to grow the raft border by
    */
    raftOutset_mm:number;
  };

}
