import * as Surplus from 'surplus'; 
import S from 's-js';

let name = S.data("Hello World")

let view = <h1>Hello {name()}!</h1>;

document.body.appendChild(view)