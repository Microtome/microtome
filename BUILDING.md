# How to build

1. Install [nvm](https://github.com/creationix/nvm), its the only sane way to manage node.js
1. I don't like using `install -g` as this installs stuff at the 'global' level of your npm install. And this may cause version conflicts with other projects or packages
1. `nvm install stable`
1. `nvm alias default stable`
1. This command will start a daughter shell and modify path to add the result of `npm bin` to it so the commands can be found
    1. `source setup_env.sh`
1. `npm install`
1. `typings install`
1. `bower install`
1. Running / Building
    1. `gulp serve` to build and serve the files locally
    1. `gulp default` build the final artifact files
