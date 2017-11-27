/**
 * Allows us to import shaders as a module so that
 * rollup can process them.
 */
declare module '*.glsl' {
    const _: string;
    export default _;
}