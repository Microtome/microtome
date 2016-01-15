module microtome.printer {

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
    // protocol: GpioProtocol | GcodeProtocol
  }
}
