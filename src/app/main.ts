import "file-saver";
import * as microtome from "microtome";
import * as THREE from "three";
import { PrinterVolumeView } from "./printerVolumeView";
import { SlicePreview } from "./slicePreview";

const PRINTER_VOLUME_DEPTH = 96;
const PRINTER_VOLUME_WIDTH = 128;
const PRINTER_VOLUME_HEIGHT = 96;

const PRINTER_X_RES = 640;
const PRINTER_Y_RES = 480;

const PrinterScene = microtome.printer.PrinterScene;

const printVolViewDiv = document.getElementById("pvview-div") as HTMLDivElement;
const slicePreviewDiv = document.getElementById("spreview-div") as HTMLDivElement;

const printerScene = new PrinterScene();
printerScene.printVolume.resize(128, 96, 96);
const sphere1 = microtome.printer.PrintMesh.fromGeometry(new THREE.SphereGeometry(10, 16, 16));
sphere1.position.set(15, 15, 15);
const sphere2 = microtome.printer.PrintMesh.fromGeometry(new THREE.SphereGeometry(10, 16, 16));
sphere2.position.set(15, 24, 20);
const sphere3 = microtome.printer.PrintMesh.fromGeometry(new THREE.SphereGeometry(15, 16, 16));
sphere3.position.set(15, 23, 35);

// Dummy printer
const printerCfg = {
  description: "Dummy Printer",
  lastModified: 0,
  name: "Dummy",
  projector: {
    xRes_px: PRINTER_X_RES,
    yRes_px: PRINTER_Y_RES,
  },
  volume: {
    depth_mm: PRINTER_VOLUME_DEPTH,
    height_mm: PRINTER_VOLUME_HEIGHT,
    width_mm: PRINTER_VOLUME_WIDTH,
  },
  zStage: {
    lead_mm: 0.1,
    microsteps: 1,
    stepsPerRev: 128,
  },
};

// Dummy job
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

// Views
new PrinterVolumeView(printVolViewDiv, printerScene);
const slicePreview = new SlicePreview(slicePreviewDiv, printerScene);

// Loader
const stlLoader = new THREE.STLLoader();

// Slice preview slider
const sliceAtSlider = document.getElementById("slice-at") as HTMLInputElement;
sliceAtSlider.min = "0";
sliceAtSlider.max = `${PRINTER_VOLUME_HEIGHT}`;
sliceAtSlider.step = "0.1";
slicePreview.sliceAt = Math.floor(PRINTER_VOLUME_HEIGHT / 4);
sliceAtSlider.value = `${slicePreview.sliceAt}`;
document.getElementById("display-mm").innerHTML = parseInt(sliceAtSlider.value, 10).toFixed(2);
sliceAtSlider.oninput = (e: Event) => {
  const sliceAt = parseFloat((e.target as HTMLInputElement).value);
  slicePreview.sliceAt = sliceAt;
  document.getElementById("display-mm").innerHTML = sliceAt.toFixed(2);
};

// Slice to file button
const sliceToFileBtn = document.getElementById("slice-to-file-btn") as HTMLButtonElement;
sliceToFileBtn.onclick = async (e: Event) => {
  e.preventDefault();
  e.stopImmediatePropagation();
  e.stopPropagation();

  sliceToFileBtn.disabled = true;

  const fileSlicer = new microtome.job.HeadlessToZipSlicerJob(printerScene, printerCfg, jobCfg);

  const blob = await fileSlicer.execute();
  saveAs(blob, `${jobCfg.name.replace(" ", "-")}-${(new Date()).toISOString()}.zip`, true);

  sliceToFileBtn.disabled = false;
};

// Load model
const fileChooserInput = document.getElementById("file-chooser") as HTMLInputElement;
fileChooserInput.onchange = () => {
  const file = fileChooserInput.files[0];
  if (!!file) {
    const fileReader = new FileReader();
    fileReader.onloadend = (loadEndEvent) => {

      const arrayBuffer = (loadEndEvent.target as any).result;

      if (file.name.endsWith(".stl")) {
        const geom = new THREE.Geometry()
          .fromBufferGeometry(stlLoader.parse(arrayBuffer));
        const mesh = microtome.printer.PrintMesh.fromGeometry(geom);
        printerScene.printObjects.push(mesh);

      } else {
        alert(`File '${file.name}' is unsupported.`);
      }
    };

    fileReader.readAsArrayBuffer(file);
  }
};
