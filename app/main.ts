import { PrinterVolumeView } from "./printerVolumeView";
import { SlicePreview } from "./slicePreview";
import * as microtome from "microtome";
import { SphereGeometry, Mesh } from "three";

let PrinterScene = microtome.three_d.PrinterScene;
let CoreMaterialsFactory = microtome.three_d.CoreMaterialsFactory;

let PrintVolViewDiv = <HTMLDivElement>document.getElementById("pvview-div")
let SlicePreviewDiv = <HTMLDivElement>document.getElementById("spreview-div")

console.log(PrintVolViewDiv);
console.log(SlicePreviewDiv);

let printerScene = new PrinterScene();
let pvv = new microtome.three_d.PrintVolumeView(320, 240, 120);
let sphere1 = new Mesh(new SphereGeometry(10, 16, 16), CoreMaterialsFactory.objectMaterial)
sphere1.position.set(15, 15, 15);
let sphere2 = new Mesh(new SphereGeometry(10, 16, 16), CoreMaterialsFactory.objectMaterial)
sphere2.position.set(15, 24, 20);
let sphere3 = new Mesh(new SphereGeometry(15, 16, 16), CoreMaterialsFactory.objectMaterial)
sphere3.position.set(15, 23, 35);

printerScene.add(pvv, sphere1, sphere2, sphere3);

var pvView = new PrinterVolumeView(PrintVolViewDiv, printerScene);
var slicePreview = new SlicePreview(SlicePreviewDiv, printerScene);