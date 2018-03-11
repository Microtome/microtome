import * as Surplus from "surplus"; { const s = Surplus; }
import SArray from "s-array";
import S from "s-js";

import "file-saver";
import * as microtome from "microtome";
import * as three from "three";
import { LABELED_INPUT } from "./labeledInput.jsx";
import { PRINTER_VOLUME_VIEW } from "./printerVolumeView.jsx";
import { SLICE_PREVIEW_VIEW } from "./slicePreview.jsx";

// import { SlicePreview } from "./slicePreview";

// Legacy three js examples expect a global THREE object
(window as any).THREE = { ...three };

/**
 * custom async require using dynamic imports
 *
 * @param path module path to load
 */
async function require(path: string) {
  await import(path);
}

void (async () => {

  // We have to await these as legacy threejs examples
  // require a global/window THREE instance to exist
  // await require("/lib/OBJLoader.js")
  await require("/lib/STLLoader.js");

  // const objLoader = new THREE.OBJLoader;
  const stlLoader = new THREE.STLLoader();

  const MICROTOME_APP_VIEW = () => {

    const PrintMesh = microtome.printer.PrintMesh;

    const PrinterScene = microtome.printer.PrinterScene;

    const materials = microtome.materials;

    const printerScene = new PrinterScene();
    printerScene.printVolume.resize(128, 96, 96);

    // Add some dummy objects
    // TODO swap to addPrintObject method
    // TODO add removePrintObject method
    const sphere1 = PrintMesh.fromGeometry(new THREE.SphereGeometry(15, 16, 16));
    sphere1.position.set(15, 15, 15);
    const sphere2 = PrintMesh.fromGeometry(new THREE.SphereGeometry(10, 16, 16));
    sphere2.position.set(15, 24, 20);
    const sphere3 = PrintMesh.fromGeometry(new THREE.SphereGeometry(5, 16, 16));
    sphere3.position.set(15, 23, 35);
    printerScene.printObjects.push(sphere1);
    printerScene.printObjects.push(sphere2);
    printerScene.printObjects.push(sphere3);

    let fileChooserInput: HTMLInputElement;
    fileChooserInput = null;

    const loadStl = (e: Event) => {
      const file = fileChooserInput.files[0];
      if (!!file) {
        const fileReader = new FileReader();
        fileReader.onloadend = (loadEndEvent) => {
          const arrayBuffer = (loadEndEvent.target as any).result;
          // var mesh:Group = null;
          // if (file.name.endsWith(".obj")) {
          //     const decoder = new TextDecoder();
          //     const objContent = decoder.decode(arrayBuffer);
          //     const group = objLoader.parse(objContent);
          //     console.log(group);
          // } else
          if (file.name.endsWith(".stl")) {
            const geom = new THREE.Geometry().
              fromBufferGeometry(stlLoader.parse(arrayBuffer));
            const mesh = PrintMesh.fromGeometry(geom);
            // mesh.position.set(15, 23, 35);
            // printerScene.
            printerScene.printObjects.push(mesh);
          } else {
            alert(`File '${file.name}' is unsupported.`);
          }
        };
        fileReader.readAsArrayBuffer(file);
      }
    };

    let sliceToFileBtn: HTMLButtonElement;
    sliceToFileBtn = null;

    const sliceToFile = async (e: Event) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      e.stopPropagation();

      sliceToFileBtn.disabled = true;

      const printerCfg = {
        description: "Dummy Printer",
        lastModified: 0,
        name: "Dummy",
        projector: {
          xRes_px: 640,
          yRes_px: 480,
        },
        volume: {
          depth_mm: 96,
          height_mm: 96,
          width_mm: 128,
        },
        zStage: {
          lead_mm: 0.1,
          microsteps: 1,
          stepsPerRev: 128,
        },
      };

      const jobCfg = {
        blankTime_ms: 500,
        description: "Dummy Slicing Job",
        layerExposureTime_ms: 8000,
        name: "Dummy Job",
        raftOutset_mm: 1.5,
        raftThickness_mm: 1.5,
        retractDistance_mm: 28,
        settleTime_ms: 5000,
        stepDistance_microns: 1,
        stepsPerLayer: 100,
        zOffset_mm: 5,
      };

      const fileSlicer = new microtome.job.HeadlessToZipSlicerJob(printerScene, printerCfg, jobCfg);

      const blob = await fileSlicer.execute();
      saveAs(blob, `${jobCfg.name.replace(" ", "-")}-${(new Date()).toISOString()}.zip`, true);

      sliceToFileBtn.disabled = false;
    };

    return <div class="flex-h" style={{ justifyContent: "space-around" }}>
      <div class="flex-v">
        <PRINTER_VOLUME_VIEW scene={printerScene}></PRINTER_VOLUME_VIEW>
        <input id="file-chooser"
          ref={fileChooserInput}
          type="file"
          onChange={loadStl}
          title="Choose File"
          placeholder="Choose File"></input>
      </div>
      <div class="flex-v">
        <SLICE_PREVIEW_VIEW scene={printerScene}></SLICE_PREVIEW_VIEW>
        <button id="slice-to-file-btn"
          ref={sliceToFileBtn}
          onClick={sliceToFile}
          style={{ width: "100px" }}>
          Slice To File
      </button>
      </div>
    </div>;
  };

  document.body.appendChild(S.root(MICROTOME_APP_VIEW));
})();
