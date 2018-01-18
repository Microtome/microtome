import { PrinterVolumeView } from "./printerVolumeView";
import { SlicePreview } from "./slicePreview";
import * as microtome from "microtome";
import * as three from "three";
import "file-saver";

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
    const stlLoader = new THREE.STLLoader;

    let PrintMesh = microtome.three_d.PrintMesh;

    let PrinterScene = microtome.three_d.PrinterScene;
    let  = microtome.three_d.;

    let PrintVolViewDiv = <HTMLDivElement>document.getElementById("pvview-div")
    let SlicePreviewDiv = <HTMLDivElement>document.getElementById("spreview-div")

    let printerScene = new PrinterScene();
    printerScene.printVolume.resize(128, 96, 96);
    let sphere1 = new PrintMesh(new THREE.SphereGeometry(10, 16, 16), objectMaterial)
    sphere1.position.set(15, 15, 15);
    let sphere2 = new PrintMesh(new THREE.SphereGeometry(10, 16, 16), objectMaterial)
    sphere2.position.set(15, 24, 20);
    let sphere3 = new PrintMesh(new THREE.SphereGeometry(15, 16, 16), objectMaterial)
    sphere3.position.set(15, 23, 35);

    // Add some dummy objects
    // TODO swap to addPrintObject method
    // TODO add removePrintObject method
    printerScene.printObjects.push(sphere1);
    printerScene.printObjects.push(sphere2);
    printerScene.printObjects.push(sphere3);

    // Views
    const pvView = new PrinterVolumeView(PrintVolViewDiv, printerScene);
    const slicePreview = new SlicePreview(SlicePreviewDiv, printerScene);

    // Slice preview slider
    const sliceAtSlider = <HTMLInputElement>document.getElementById("slice-at");
    sliceAtSlider.min = "0";
    sliceAtSlider.max = "96";
    sliceAtSlider.step = "0.1";
    sliceAtSlider.value = "25";
    slicePreview.sliceAt = 25;
    document.getElementById("display-mm").innerHTML = parseInt(sliceAtSlider.value, 10).toFixed(2);
    sliceAtSlider.oninput = (e: Event) => {
        let sliceAt = parseFloat((<HTMLInputElement>e.target).value)
        slicePreview.sliceAt = sliceAt
        document.getElementById("display-mm").innerHTML = sliceAt.toFixed(2);
    };

    // Slice to file
    const sliceToFileBtn = <HTMLButtonElement>document.getElementById("slice-to-file-btn");
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

        const blob = await fileSlicer.execute();
        saveAs(blob, `${jobCfg.name.replace(" ", "-")}-${(new Date).toISOString()}.zip`, true)

        sliceToFileBtn.disabled = false;
    }

    // Load model
    const fileChooserInput = <HTMLInputElement>document.getElementById("file-chooser");
    fileChooserInput.onchange = (e: Event) => {
        const file = fileChooserInput.files[0];
        if (!!file) {
            const fileReader = new FileReader();
            fileReader.onloadend = (e) => {
                const arrayBuffer = (<any>e.target).result;
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
                    let mesh = new PrintMesh(geom, objectMaterial)
                    // mesh.position.set(15, 23, 35);
                    // printerScene.
                    printerScene.add(mesh);
                } else {
                    alert(`File '${file.name}' is unsupported.`)
                }
            };
            fileReader.readAsArrayBuffer(file);
        }
    }
})();
