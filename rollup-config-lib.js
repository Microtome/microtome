import string from 'rollup-plugin-string';

export default {
    entry: 'build/lib/js/index.js',
    format: 'umd',
    external: ['THREE'],
    dest: 'dist/lib/microtome-lib.js',
    sourceMap: true,
    moduleName: "microtome",
    plugins: [
        // customResolver(),
        string({
            // Required to be specified
            include: 'lib/**/*.glsl',
        })
    ]
};