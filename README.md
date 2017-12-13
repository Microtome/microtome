# Microtome

Microtome is a gpu accelerated model slicer intended for generating slice images for
use in DLP style 3D printers. It is NOT intended for SLA style printers. It does not
produce vector images, but bitmaps, using shader and scene tricks.

# Status

I've spent the past several weeks, cleaning up, simplifying, and updating three.js
dependencies. The old polymer app was a time sink, and discarded for a much simpler
vanillajs demo app whose sole purpose is to test the library and serve as an example. 
Build process was simplified.

The scope has been shrunk to be 'just a library' in the near term. 

## What works?

- Basic Slicing
  - It's FAST! Can slice at 2560 x 1920 at ~100ms per slice on Chrome, and ~200ms
  on FireFox. 500 slices takes about 50s on chrome.
- Robust, handles self-intersecting and intersecting geometry.
  - You don't need to 'union' your meshes with support geometry before slicing,
  or union pieces together.
- Volume estimation
  - Please note that large overlap in intersecting geometry, or shelling will throw this off, 
  but it will give you a worst case estimate.
- Slice job support. Just feed it some configuration and away you go!
  - Will peg your cpu and may cause browser lag, but goes fast.


## Future Directions

- Mesh union
- Mesh repair utilities
- Support generation utilities
- Shelling at slice time.
- Exposure masks for brightness leveling ( simple radial fill, and custom images )
- Multi-exposure per slice to reduce cooking/heating of resin via cure heat

## Donations

Donations are always accepted, and will be used for coffee, hardware costs, hosting, and other fees

Bitcoin: 1LsbziuCYKyCY3Urd3Yo7WSsK1Co6wjCqT

Paypal: paypal.me/dajoyce

