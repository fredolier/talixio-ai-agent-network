export { cn } from "cn"

/** Lit une table par une clé venue d'ailleurs, sans jamais tomber sur une propriété héritée. */
export function pick<T>(table: Record<string, T>, key: unknown): T | undefined {
  return typeof key === 'string' && Object.hasOwn(table, key) ? table[key] : undefined
}
