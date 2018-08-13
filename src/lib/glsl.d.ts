/**
 * Allows us to import shaders as a module so that
 * rollup can process them.
 */
declare module '*.glsl' {
    const value: string;
    export default value;
}