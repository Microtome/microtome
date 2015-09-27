module microtme.serial_support{

    export function is_serial_available():boolean{
      if(chrome.serial) return true;
      return false;
    }

    // export function open_serial_connection():chrome.serial.ConnectionInfo{
    //
    // }

}
