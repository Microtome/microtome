import rollupString from 'rollup-plugin-string';
import sourcemaps from 'rollup-plugin-sourcemaps';

export default {
    input: 'build/lib/microtome/index.js',
    output:{
        name: "microtome",
        format: 'es',
        file: 'dist/lib/microtome.js',
        paths: {
            "three": "/lib/three",
            "jszip": "/lib/jszip",
            "file-saver": "/lib/file-saver"
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
