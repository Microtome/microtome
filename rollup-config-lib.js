import string from 'rollup-plugin-string';

export default{
    entry: 'lib/ts/index.js',
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