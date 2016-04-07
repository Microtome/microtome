# How to build

1. Install [nvm](https://github.com/creationix/nvm), its the only sane way to manage node.js
1. I don't like using `install -g` as this installs stuff at the 'global' level of your npm install. And this may cause version conflicts with other projects or packages
1. `nvm install stable`
1. `nvm alias default stable`
1. `` `npm bin`/npm install``
1. `` `npm bin`/typings install``
1. `` `npm bin`/bower install``
1. `` `npm bin`/gulp serve`` to build and serve the files locally
1. `` `npm bin`/gulp default`` build the final artifact files
