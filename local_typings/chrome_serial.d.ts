/////////////////////////////////////////////
// Serial
// Ambient types for Chrome serial port API
////////////////////////////////////////////

declare module chrome.serial {

  interface ConnectionOptions {
    persistent?: boolean;
    name?: String;
    /** Size of buffer in bytes, integer */
    bufferSize?: number;
    /** Bit rate, integer */
    bitRate?: number;
    /**
      "seven"
      "eight"
    */
    dataBits?: string;
    /**
      "no"
      "odd"
      "even"
    */
    parityBit?: string;
    /**
      "one"
      "two"
    */
    stopBits?: string;
    /** Use CTS Flow control? */
    ctsFlowControl?: boolean;
    /** Milliseconds, should be int */
    receiveTimeout?: number;
    /** Milliseconds, should be int */
    sendTimeout?: number;
  }

  interface ConnectionInfo {
    connectionId: number;
    paused: boolean;
    persistent: boolean;
    name: String;
    bufferSize: number;
    receiveTimeout: number;
    sendTimeout: number;
    bitRate?: number;
    /**
      "seven"
      "eight"
    */
    dataBits?: string;
    /**
      "no"
      "odd"
      "even"
    */
    parityBit?: string;
    /**
      "one"
      "two"
    */
    stopBits?: string;
    ctsFlowControl?: boolean;
  }

  interface DeviceInfo {
    path: String;
    vendorId?: number;
    productId?: number;
    displayName?: String;
  }

  interface SendInfo {
    bytesSent: number;
    /**
      "disconnected"
      "pending"
      "timeout"
      "system_error"
    */
    error?: string;
  }

  interface ControlSignals {
    dcd: boolean;
    cts: boolean;
    ri: boolean;
    dsr: boolean;
  }

  /**
   Get available serial devices
   */
  export function getDevices(callback: (deviceInfos: [DeviceInfo]) => void): void;
  /**
  Connect to a device
  */
  export function connect(path: String, options: ConnectionOptions, callback: (connectionInfo: ConnectionInfo) => void): void
  /**
  Update the communication settings for a existing open device
  */
  export function update(connectionId: number, options: ConnectionOptions, callback: (result: boolean) => void): void
  /**
  Close connection and free up the serial port
  */
  export function disconnect(connectionId: number, callback: (result: boolean) => void): void
  /**
  Pause connection
  */
  export function setPaused(connectionId: number, paused: boolean, callback: () => void): void
  /**
  Get connection information
  */
  export function getInfo(connectionId: number, callback: (connectionInfo: ConnectionInfo) => void): void
  /**
  Get all open connections in use by application
  */
  export function getConnections(callback: (connectionInfos: [ConnectionInfo]) => void): void
  /**
  Send data
  */
  export function send(connectionId: number, data: ArrayBuffer, callback: (sendInfo: SendInfo) => void): void
  /**
  Flush
  */
  export function flush(connectionId: number, callback: (result: boolean) => void): void
  /**
  Get current status of control signals
  */
  export function getControlSignals(connectionId: number, callback: (signals: ControlSignals) => void): void
  /**
  Set control signals
  */
  export function setControlSignals(connectionId: number, signals: ControlSignals, callback: (result: boolean) => void): void
  /**
  Set break
  */
  export function setBreak(connectionId: number, callback: (result: boolean) => void): void
  /**
  Clear break
  */
  export function clearBreak(connectionId: number, callback: (result: boolean) => void): void

  interface Event<T> {
    addListener(callback: (info: T) => void): void;
  }

  interface ReceiveEventArgs {
    connectionId: number;
    data: ArrayBuffer;
  }

  interface ReceiveErrorEventArgs {
    connectionId: number;
    /**
      "disconnected"
      "device_lost"
      "timeout"
      "break"
      "frame_error"
      "overun"
      "buffer_overflow"
      "parity_error"
      "system_error"
    */
    error: string;
  }

  var onReceive: Event<ReceiveEventArgs>;
  var onReceiveError: Event<ReceiveErrorEventArgs>;
}
