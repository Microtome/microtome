# Packages and ideas to use

- https://github.com/vangware/fontawesome-iconset

- Apparently a company has released a polymer that can cure via the light a LCD emits
  - http://www.photocentric3d.com/#!blank/thd1s

- Distance functions for 3d support textures
  - http://iquilezles.org/www/articles/distfunctions/distfunctions.htm

- Three.js EffectComposer
  - https://www.airtightinteractive.com/2013/02/intro-to-pixel-shaders-in-three-js/

http://stackoverflow.com/questions/22520412/reading-data-from-three-js-rendertotarget-gives-unexpected-results

Use readpixels to force a fence ensuring rendering is done. This will matter more
when rendering is forced off into web workers.

Looks like we can't use webworkers immeadiately, chrome has not implemented offscreen canvas

http://codeflow.org/issues/timing/

Look at this for how to use readpixels. Looks like you can read just 1 pixel worth of bytes.

Should use adaptive slice timing to back off or ramp up to maximize slice speed.
Save what works best in local storage...
