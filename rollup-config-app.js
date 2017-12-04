import rollupString from 'rollup-plugin-string';
import sourcemaps from 'rollup-plugin-sourcemaps';

export default {
    entry: 'build/app/main.js',
    format: 'es',
    external: ['three', 'jszip', 'microtome'],
    dest: 'dist/app/main.js',
    sourceMap: true,
    moduleName: "main",
    plugins: [
        // Relative Paths module 
        // TODO BRING IN!!!
        sourcemaps()
    ],
    paths: {
        "three": "/lib/js/three",
        "jszip": "/lib/js/jszip",
        "microtome": "/lib/js/microtome"
    }
};
