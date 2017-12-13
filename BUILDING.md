# How to build

## Dependencies

1. Install [nvm](https://github.com/creationix/nvm), its the only sane way to manage node.js
1. I don't like using `install -g` as this installs stuff at the 'global' level of your npm install. And this may cause version conflicts with other projects or packages
1. `nvm install stable`
1. `nvm alias default stable`
1. `npm install yarn` cuz npm sucks. [Yarn](https://yarnpkg.com/) is faster.
1. `yarn` to install dependencies


## Running / Building

see package.json for commands

* `yarn run clean` to remove build/ and dist/
* `yarn run build:lib` to build app and lib and copy to /build/lib/microtome
* `yarn run dist:lib` to bundle lib with shaders as es6 module under dist/lib/microtome.js
* `npm serve:hot` which will build the library, and sample app, and server on localhost:8080 with hot reloading of all ts, html, and css edits
