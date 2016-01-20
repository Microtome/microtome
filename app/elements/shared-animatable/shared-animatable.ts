
@component("shared-animatable")
@behavior(Polymer.NeonSharedElementAnimatableBehavior)
class SharedAnimatable extends polymer.Base {

  @property({type:Object})
  public sharedElements: Object = {};

  @property({type:Object})
  public animationConfig: Object = {};
}

SharedAnimatable.register();
