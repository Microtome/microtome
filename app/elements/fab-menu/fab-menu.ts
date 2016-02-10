
@component("fab-menu")
@behavior("Polymer.NeonAnimationRunnerBehavior")

class FabMenu extends polymer.Base {

  /**
  * Direction menu expands
  * Values are right, left, up, down
  */
  @property({ readOnly: false, notify: true, type: String, value: () => "up" })
  direction: String;

  @property({ readOnly: false, notify: false, type: String })
  closedIcon: String = "menu";

  @property({ readOnly: false, notify: false, type: String })
  openedIcon: String = "add";

  @property({ readOnly: false, notify: false })
  onOpenedClicked: Function;

  private _layoutClasses(dir: String) {
    window.console.log(dir);
    if (dir === "down") {
      return "layout vertical center";
    } else if (dir === "left") {
      return "layout horizontal-reverse center";
    } else if (dir === "right") {
      return "layout horizontal center";
    } else {
      return "layout vertical-reverse center";
    }
  }

  private _openMenu(e: Event) {
    window.console.log("OPEN");
    this.$["fab-menu-opened"].hidden=false;
    this.$["fab-menu-closed"].hidden=true;
  }

  // @property({})
  // autoClose: boolean = false;

  public attached() {
  }

}

FabMenu.register();
