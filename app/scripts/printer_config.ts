module microtome.printer {

  export enum GpioPinUse {

  }

  /**
  * For printers driven directly via GPIO, descrbes what each pin does
  */
  export interface GpioProtocol {

  }

  /**
  * Configuration for GCode driven printers
  */
  export interface GcodeProtocol {
  }

  export enum ThreadUnits {
    TPI, PITCH_MM, PITCH_IN
  }

  export interface ZStage {
    threadMeasure: number,
    threadUnits: ThreadUnits,
    stepsPerRev: number,
    microsteps: number
  }

  /**
  * Specifies printer volume
  */
  export interface PrintVolume {
    width: number,
    depth: number,
    height: number
  }

  /**
  * Printer configuration class
  */
  export interface PrinterConfig {
    /** name of printer */
    name: string;
    /** description of printer */
    description: string;
    /** ms since epoch */
    lastModified: number;
    /**
    * Print volume
    * x = width
    * y = depth
    * z = height
    */
    volume: PrintVolume;

    // zStage: ZStage

    // protocol: GpioProtocol | GcodeProtocol
  }

  /**
  * Print job settings
  */
  export interface PrintJobSettings {
    /** Name of settings */
    name: string;
    /** Layer thickness, mm */
    layerThickness: number;
    /** Settle time, seconds */
    settleTime: number;
    /** Exposure time per layer, ms */
    layerExposureTime: number;
    /** Blank time, ms */
    blankTime: number;
    /** Retract distance, distance platforms move to peel print, mm */
    retractDistance: number
  }
  
}
