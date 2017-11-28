
@component("fab-radial")
@behavior("Polymer.NeonAnimationRunnerBehavior")
class FabRadial extends polymer.Base {

  public open(x: number, y: number): void {
    this.$['radial-menu'].hidden = false;
    var r = this.$['radial-menu'].getBoundingClientRect();
    this.$['radial-menu'].style.top = ("" + (y - r.height / 2) + "px");
    this.$['radial-menu'].style.left = ("" + (x - r.width / 2) + "px");
  }

  public close(): void {
    this.$['radial-menu'].hidden = true;
  }

  public onContextMenu(e: MouseEvent): boolean {
    e.preventDefault();
    return false;
  }

}

FabRadial.register();
