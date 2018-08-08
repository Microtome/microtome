/** 
 * Copy all dependencies into a final directory for publishing 
 */

function errHandler(err) {
  if(err){
    console.error(err);
  }
}

const fs = require('fs');
const path = require('path');

fs.mkdirSync("./public");
fs.mkdirSync("./public/lib");
fs.mkdirSync("./public/app");

fs.copyFile("./node_modules/three-full/builds/Three.es.js","./public/lib/three-es6", errHandler);
fs.copyFile("./node_modules/jszip/dist/jszip.js", "./public/lib/jszip.js", errHandler);
fs.copyFile("./node_modules/file-saver/FileSaver.js", "./public/lib/file-saver.js", errHandler);
fs.copyFile("./dist/lib/microtome.js", "./public/lib/microtome.js", errHandler);
fs.copyFile("./dist/app/main.js", "./public/app/main.js", errHandler);

fs.copyFile("./app/index.html", "./public/index.html", errHandler);

