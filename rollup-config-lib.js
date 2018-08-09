import rollupString from 'rollup-plugin-string';
import sourcemaps from 'rollup-plugin-sourcemaps';

export default {
    input: 'build/lib/microtome/index.js',
    output:{
        name: "microtome",
        format: 'es',
        file: 'dist/lib/microtome.js',
        paths: {
            "three": "/lib/three-full.js",
            "jszip": "/lib/jszip.js",
            "file-saver": "/lib/file-saver.js"
        },
        sourcemap: true,
    },
    external: ['three', 'jszip'],
    plugins: [
        rollupString({
            // Required to be specified
            include: 'build/lib/microtome/**/*.glsl',
        }),
        sourcemaps()
    ],
};
