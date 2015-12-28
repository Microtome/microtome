var __extends = (this && this.__extends) || function (d, b) {
    for (var p in b) if (b.hasOwnProperty(p)) d[p] = b[p];
    function __() { this.constructor = d; }
    d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
};
var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};
var MyDerpderp = (function (_super) {
    __extends(MyDerpderp, _super);
    function MyDerpderp() {
        _super.apply(this, arguments);
        this.greet = "Hello";
    }
    MyDerpderp.prototype.greetChanged = function (newValue, oldValue) {
        console.log("greet has changed from " + oldValue + " to " + newValue);
    };
    MyDerpderp.prototype.greetAll = function (greet) {
        return this.greet + " to all";
    };
    MyDerpderp.prototype.handleClick = function (e) {
        this.greet = "Holà";
        this.fire("greet-event");
    };
    MyDerpderp.prototype.onButtonWasClicked = function (e) {
        console.log('event "greet-event" received');
    };
    MyDerpderp.prototype.ready = function () {
        console.log(this['is'], "ready!");
    };
    MyDerpderp.prototype.created = function () { };
    MyDerpderp.prototype.attached = function () { };
    MyDerpderp.prototype.detached = function () { };
    __decorate([
        property({ type: String })
    ], MyDerpderp.prototype, "greet", void 0);
    __decorate([
        observe("greet")
    ], MyDerpderp.prototype, "greetChanged", null);
    __decorate([
        computed()
    ], MyDerpderp.prototype, "greetAll", null);
    __decorate([
        listen("greet-event")
    ], MyDerpderp.prototype, "onButtonWasClicked", null);
    MyDerpderp = __decorate([
        component('my-derpderp')
    ], MyDerpderp);
    return MyDerpderp;
})(polymer.Base);
MyDerpderp.register();

//# sourceMappingURL=my-derpderp.js.map
