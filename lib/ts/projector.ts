
/// Setup projector display page and handle resize.

var canvas: HTMLCanvasElement = <HTMLCanvasElement>document.querySelector('#slice');

var resizeHandler = function(){
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
}

var requestFullScreen = function(){
  var el = document.documentElement;
  var fs = el.requestFullscreen || el.webkitRequestFullscreen;
  fs.call(el);
}

window.onclick = requestFullScreen;

window.onresize = resizeHandler;

window.onload = resizeHandler;
