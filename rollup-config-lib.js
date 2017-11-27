import rollupString from 'rollup-plugin-string';

export default {
    entry: 'build/lib/js/index.js',
    format: 'es',
    external: ['three', 'jszip'],
    dest: 'dist/lib/microtome-lib.js',
    sourceMap: true,
    moduleName: "microtome",
    plugins: [
        // customResolver(),
        rollupString({
            // Required to be specified
            include: 'build/lib/js/**/*.glsl',
        })
    ],
    paths: {
    }
};
