import { PrinterVolumeView } from "./printerVolumeView";
import { SlicePreview } from "./slicePreview";
import * as microtome from "microtome";
import { SphereGeometry, Mesh } from "three";

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
printerScene.printObjects.push(sphere1);
printerScene.printObjects.push(sphere2);
printerScene.printObjects.push(sphere3);

var pvView = new PrinterVolumeView(PrintVolViewDiv, printerScene);
var slicePreview = new SlicePreview(SlicePreviewDiv, printerScene);
var sliceAtSlider = <HTMLInputElement>document.getElementById("slice-at");
sliceAtSlider.min = "0";
sliceAtSlider.max = "96";
sliceAtSlider.step = "0.1";
sliceAtSlider.value = "25";
slicePreview.sliceAt = 25;
sliceAtSlider.oninput = (e: Event) => {
    slicePreview.sliceAt = parseInt((<HTMLInputElement>e.target).value, 10)
};


