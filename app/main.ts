import { PrinterVolumeView } from "./printerVolumeView";
import { SlicePreview } from "./slicePreview";
import * as microtome from "microtome";
import { SphereGeometry, Mesh } from "three";
import "file-saver";

let PrintMesh = microtome.three_d.PrintMesh;

let PrinterScene = microtome.three_d.PrinterScene;
let CoreMaterialsFactory = microtome.three_d.CoreMaterialsFactory;

let PrintVolViewDiv = <HTMLDivElement>document.getElementById("pvview-div")
let SlicePreviewDiv = <HTMLDivElement>document.getElementById("spreview-div")

console.log(PrintVolViewDiv);
console.log(SlicePreviewDiv);

let printerScene = new PrinterScene();
printerScene.printVolume.resize(128, 96, 96);
let sphere1 = new PrintMesh(new SphereGeometry(10, 16, 16), CoreMaterialsFactory.objectMaterial)
sphere1.position.set(15, 15, 15);
let sphere2 = new PrintMesh(new SphereGeometry(10, 16, 16), CoreMaterialsFactory.objectMaterial)
sphere2.position.set(15, 24, 20);
let sphere3 = new PrintMesh(new SphereGeometry(15, 16, 16), CoreMaterialsFactory.objectMaterial)
sphere3.position.set(15, 23, 35);

// Add some dummy objects
// TODO swap to addPrintObject method
// TODO add removePrintObject method
printerScene.printObjects.push(sphere1);
printerScene.printObjects.push(sphere2);
printerScene.printObjects.push(sphere3);

const pvView = new PrinterVolumeView(PrintVolViewDiv, printerScene);
const slicePreview = new SlicePreview(SlicePreviewDiv, printerScene);
const sliceAtSlider = <HTMLInputElement>document.getElementById("slice-at");
sliceAtSlider.min = "0";
sliceAtSlider.max = "96";
sliceAtSlider.step = "0.1";
sliceAtSlider.value = "25";
slicePreview.sliceAt = 25;
sliceAtSlider.oninput = (e: Event) => {
    slicePreview.sliceAt = parseInt((<HTMLInputElement>e.target).value, 10)
};

const sliceToFileBtn = <HTMLButtonElement> document.getElementById("slice-to-file-btn");
sliceToFileBtn.onclick = async (e: Event) => {
    e.preventDefault();
    e.stopImmediatePropagation();
    e.stopPropagation();
    
    sliceToFileBtn.disabled = true;

    const printerCfg = {
        name: "Dummy",
        description: "Dummy Printer",
        lastModified: 0,
        volume: {
            width_mm: 128,
            height_mm: 96,
            depth_mm: 96
        },
        zStage: {
            lead_mm: 0.1,
            stepsPerRev: 128,
            microsteps: 1
        },
        projector: {
            xRes_px: 640,
            yRes_px: 480
        }
    };

    const jobCfg = {
        name: "Dummy Job",
        description: "Dummy Slicing Job",
        stepDistance_microns: 1,
        stepsPerLayer: 100,
        settleTime_ms: 5000,
        layerExposureTime_ms: 8000,
        blankTime_ms: 500,
        retractDistance_mm: 28,
        zOffset_mm: 5,
        raftThickness_mm: 1.5,
        raftOutset_mm: 1.5
    }

    const fileSlicer = new microtome.slicer_job.HeadlessToZipSlicerJob(printerScene, printerCfg, jobCfg);

    let jobStart = Date.now();
    const blob = await fileSlicer.execute();
    console.log(`Job took ${((Date.now()-jobStart)/1000)} seconds`)
    saveAs(blob,`${jobCfg.name.replace(" ","-")}-${(new Date).toISOString()}.zip`,true)

    sliceToFileBtn.disabled = false;    
} 
