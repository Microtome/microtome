# How to build

## Dependencies

1. Install [nvm](https://github.com/creationix/nvm), its the only sane way to manage node.js
1. I don't like using `install -g` as this installs stuff at the 'global' level of your npm install. And this may cause version conflicts with other projects or packages
1. `nvm install stable`
1. `nvm alias default stable`
1. `npm install yarn` cuz npm sucks. [Yarn](https://yarnpkg.com/) is faster.
1. This command will start a daughter shell and modify path to add the result of `npm bin` to it so the commands can be found
    1. `source setup_env.sh`
1. `yarn`
1. `typings install`
1. `bower install`

## Running / Building

**Under active development as things are quickly changing right now**

see package.json for commands

* `npm run clean` to remove build/ and dist/
* `npm run build` to build app and lib and copy to /build
* `npm run serve:dev` to serve from build/
* `npm dev:hot` which will build and then serve from build/, recompiling as needed as files are edited or changed.
* dist target is not finished yet
