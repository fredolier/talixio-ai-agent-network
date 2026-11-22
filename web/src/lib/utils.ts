export { cn } from "cn"

/** Reads a table by a key that came from elsewhere, never landing on an inherited property. */
export function pick<T>(table: Record<string, T>, key: unknown): T | undefined {
  return typeof key === 'string' && Object.hasOwn(table, key) ? table[key] : undefined
}
