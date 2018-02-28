import sourcemaps from 'rollup-plugin-sourcemaps';

export default {
  input: 'build/app/main.js',
  output: {
    name: "main",
    format: 'es',
    external: ['three', 'jszip', 'microtome'],
    paths: {
      "@three/loaders/OBJLoader.js": "/lib/OBJLoader.js",
      "@three/loaders/STLLoader.js": "/lib/STLLoader.js",
      "three": "/lib/three.js",
      "jszip": "/lib/jszip.js",
      "microtome": "/lib/microtome.js",
      "file-saver": "/lib/file-saver.js",
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
