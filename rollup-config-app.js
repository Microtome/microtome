import sourcemaps from 'rollup-plugin-sourcemaps';

export default {
    input: 'build/app/main.js',
    output:{
        name: "main",
        format: 'es',
        external: ['three', 'jszip', 'microtome'],
        paths: {
            "@three/loaders/OBJLoader.js": "/lib/OBJLoader.js",
            "@three/loaders/STLLoader.js": "/lib/STLLoader.js",
            "three": "/lib/three",
            "jszip": "/lib/jszip",
            "microtome": "/lib/microtome",
            "file-saver": "/lib/file-saver",
            "lib": "/lib:"
        },
        sourcemap: true,
        file: 'dist/app/main.js',
    },
    plugins: [
        sourcemaps()
    ],
    experimentalDynamicImport: true

};
