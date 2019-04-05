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
* `yarn run build:lib` to build library and copy to dist/lib
* `yarn run build:app` to build a bundled app including all assets under dist:app
* `npm serve:hot` run parcel js in hot reload mode.
