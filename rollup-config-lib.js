import rollupString from 'rollup-plugin-string';
import sourcemaps from 'rollup-plugin-sourcemaps';

export default {
    entry: 'build/lib/microtome/index.js',
    format: 'es',
    external: ['three', 'jszip'],
    dest: 'dist/lib/microtome.js',
    sourceMap: true,
    moduleName: "microtome",
    plugins: [
        rollupString({
            // Required to be specified
            include: 'build/lib/microtome/**/*.glsl',
        }),
        sourcemaps()
    ],
    paths: {
        "three": "/lib/three",
        "jszip": "/lib/jszip",
        "file-saver": "/lib/file-saver"
    }
};
