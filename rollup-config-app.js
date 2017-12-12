import sourcemaps from 'rollup-plugin-sourcemaps';

export default {
    entry: 'build/app/main.js',
    format: 'es',
    external: ['three', 'jszip', 'microtome'],
    dest: 'dist/app/main.js',
    sourceMap: true,
    moduleName: "main",
    plugins: [
        sourcemaps()
    ],
    paths: {
        "three": "/lib/three",
        "jszip": "/lib/jszip",
        "microtome": "/lib/microtome",
        "file-saver": "/lib/file-saver"
    }
};
