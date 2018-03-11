import sourcemaps from 'rollup-plugin-sourcemaps';
import surplus from 'rollup-plugin-surplus';
import resolve from 'rollup-plugin-node-resolve';

export default [
//   {
//   input: 'build/app/main.js',
//   output: {
//     name: "main",
//     format: 'es',
//     external: ['three', 'jszip', 'microtome'],
//     paths: {
//       "@three/loaders/OBJLoader.js": "/lib/OBJLoader.js",
//       "@three/loaders/STLLoader.js": "/lib/STLLoader.js",
//       "three": "/lib/three.js",
//       "jszip": "/lib/jszip.js",
//       "microtome": "/lib/microtome.js",
//       "file-saver": "/lib/file-saver.js",
//       "lib": "/lib:"
//     },
//     sourcemap: true,
//     file: 'dist/app/main.js',
//   },
//   plugins: [
//     // resolve({
//     //   module: true,
//     //   jsnext: true,
//     //   main: true,
//     //   browser: true,
//     //   extensions: ['.js', '.json'],
//     //   modulesOnly: true,
//     // }),
//     sourcemaps()
//   ],
//   experimentalDynamicImport: true
// }, 
{
  input: 'build/app/components/microtomeApp.jsx',
  external: ['three', 'jszip', 'microtome'],
  output: {
    name: "microtomeApp.js",
    format: 'es',
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
    file: 'dist/app/microtomeApp.js',
  },
  plugins: [
    resolve({
      module: true,
      jsnext: true,
      main: true,
      browser: true,
      extensions: ['.js', '.json', '.jsx'],
      modulesOnly: true,
    }),
    surplus(),
    sourcemaps()
  ],
  experimentalDynamicImport: true
}];
