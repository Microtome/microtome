import "file-saver";
import * as microtome from "microtome";
import * as three from "three";
import { PrinterVolumeView } from "./printerVolumeView";
import { SlicePreview } from "./slicePreview";

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

  const PrintMesh = microtome.printer.PrintMesh;

  const PrinterScene = microtome.printer.PrinterScene;

  const materials = microtome.materials;

  const printVolViewDiv = document.getElementById("pvview-div") as HTMLDivElement;
  const slicePreviewDiv = document.getElementById("spreview-div") as HTMLDivElement;

  const printerScene = new PrinterScene();
  printerScene.printVolume.resize(128, 96, 96);
  const sphere1 = new PrintMesh(new THREE.SphereGeometry(10, 16, 16), materials.objectMaterial);
  sphere1.position.set(15, 15, 15);
  const sphere2 = new PrintMesh(new THREE.SphereGeometry(10, 16, 16), materials.objectMaterial);
  sphere2.position.set(15, 24, 20);
  const sphere3 = new PrintMesh(new THREE.SphereGeometry(15, 16, 16), materials.objectMaterial);
  sphere3.position.set(15, 23, 35);

  // Add some dummy objects
  // TODO swap to addPrintObject method
  // TODO add removePrintObject method
  printerScene.printObjects.push(sphere1);
  printerScene.printObjects.push(sphere2);
  printerScene.printObjects.push(sphere3);

  // Views
  const pvView = new PrinterVolumeView(printVolViewDiv, printerScene);
  const slicePreview = new SlicePreview(slicePreviewDiv, printerScene);

  // Slice preview slider
  const sliceAtSlider = document.getElementById("slice-at") as HTMLInputElement;
  sliceAtSlider.min = "0";
  sliceAtSlider.max = "96";
  sliceAtSlider.step = "0.1";
  sliceAtSlider.value = "25";
  slicePreview.sliceAt = 25;
  document.getElementById("display-mm").innerHTML = parseInt(sliceAtSlider.value, 10).toFixed(2);
  sliceAtSlider.oninput = (e: Event) => {
    const sliceAt = parseFloat((e.target as HTMLInputElement).value);
    slicePreview.sliceAt = sliceAt;
    document.getElementById("display-mm").innerHTML = sliceAt.toFixed(2);
  };

  // Slice to file
  const sliceToFileBtn = document.getElementById("slice-to-file-btn") as HTMLButtonElement;
  sliceToFileBtn.onclick = async (e: Event) => {
    e.preventDefault();
    e.stopImmediatePropagation();
    e.stopPropagation();

    sliceToFileBtn.disabled = true;

    const printerCfg = {
      description: "Dummy Printer",
      lastModified: 0,
      name: "Dummy",
      projector: {
        xRes_px: 4 * 640,
        yRes_px: 4 * 480,
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

  // Load model
  const fileChooserInput = document.getElementById("file-chooser") as HTMLInputElement;
  fileChooserInput.onchange = (e: Event) => {
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
          const mesh = new PrintMesh(geom, materials.objectMaterial);
          // mesh.position.set(15, 23, 35);
          // printerScene.
          printerScene.add(mesh);
        } else {
          alert(`File '${file.name}' is unsupported.`);
        }
      };
      fileReader.readAsArrayBuffer(file);
    }
  };
})();
