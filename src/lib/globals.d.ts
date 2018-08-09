/**
 * TypeScript global declarations go here
 */

declare global {
  // Defined in es2017 but most major browsers support it
  interface String {
    padStart(width: number, pad: string): string
  }
}

export { }