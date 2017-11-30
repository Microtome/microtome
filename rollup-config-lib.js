import rollupString from 'rollup-plugin-string';
import sourcemaps from 'rollup-plugin-sourcemaps';

export default {
    entry: 'build/lib/js/microtome/index.js',
    format: 'es',
    external: ['three', 'jszip'],
    dest: 'dist/lib/microtome.js',
    sourceMap: true,
    moduleName: "microtome",
    plugins: [
        // customResolver(),
        rollupString({
            // Required to be specified
            include: 'build/lib/js/microtome/**/*.glsl',
        }),
        sourcemaps()
    ],
    paths: {
        "three": "/lib/js/three",
        "jszip": "/lib/js/jszip"
    }
};
