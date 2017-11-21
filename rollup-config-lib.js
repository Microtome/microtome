import rollupString from 'rollup-plugin-string';

export default {
    entry: 'build/lib/js/index.js',
    format: 'umd',
    external: ['THREE'],
    dest: 'dist/lib/microtome-lib.js',
    sourceMap: true,
    moduleName: "microtome",
    plugins: [
        // customResolver(),
        rollupString({
            // Required to be specified
            include: 'lib/**/*.glsl',
        })
    ]
};
