
@component("microtome-app")
class MicrotomeApp extends polymer.Base{

    onMenuActivate(event: Event, detail: Object) {
      var menu = Polymer.dom(event).localTarget;
      menu.select(-1);
    }

    ready()
    {
      console.log( this['is'], "ready!")
    }

}

MicrotomeApp.register();
