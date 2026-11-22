// Checks that every text given to t() or tn() in the code has a French translation,
// and that no translation is left without a text. Run by `npm run build`.
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'

const root = new URL('../src/', import.meta.url).pathname
const files = (dir) =>
  readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    return statSync(path).isDirectory() ? files(path) : /\.tsx?$/.test(name) ? [path] : []
  })

const literal = String.raw`'((?:[^'\\\n]|\\.)*)'|"((?:[^"\\\n]|\\.)*)"`
const unquote = (single, double) => (single ?? double).replace(/\\(.)/g, '$1')
const used = new Map()
const dynamic = []
for (const file of files(root)) {
  if (file.endsWith('lib/i18n.ts')) continue
  const source = readFileSync(file, 'utf8')
  const where = file.slice(root.length)
  for (const m of source.matchAll(new RegExp(String.raw`(?<![\w.])t\(\s*(?:${literal})`, 'g'))) used.set(unquote(m[1], m[2]), where)
  for (const m of source.matchAll(new RegExp(String.raw`(?<![\w.])tn\(\s*[^,]+,\s*(?:${literal})\s*,\s*(?:${literal})`, 'g'))) {
    used.set(unquote(m[1], m[2]), where)
    used.set(unquote(m[3], m[4]), where)
  }
  // texts kept in tables and translated later: marked with tx('…')
  for (const m of source.matchAll(new RegExp(String.raw`(?<![\w.])tx\(\s*(?:${literal})`, 'g'))) used.set(unquote(m[1], m[2]), where)
  for (const m of source.matchAll(/(?<![\w.])tn?\(\s*`/g)) dynamic.push(`${where}: ${source.slice(m.index, m.index + 60).split('\n')[0]}`)
}

const dir = join(root, 'locales/fr')
const fr = new Map()
let failed = false
for (const name of readdirSync(dir).filter((n) => n.endsWith('.json'))) {
  for (const [key, value] of Object.entries(JSON.parse(readFileSync(join(dir, name), 'utf8')))) {
    if (fr.has(key) && fr.get(key) !== value) {
      console.error(`fr: two translations for « ${key} » (${name})`)
      failed = true
    }
    fr.set(key, value)
    const holes = (s) => [...s.matchAll(/\{\w+\}/g)].map((h) => h[0]).sort().join()
    if (holes(key) !== holes(value)) {
      console.error(`fr: placeholders differ for « ${key} » (${name})`)
      failed = true
    }
  }
}
for (const line of dynamic) {
  console.error(`template literal given to t(): ${line}`)
  failed = true
}
for (const [key, where] of used) {
  if (!fr.has(key)) {
    console.error(`fr: missing « ${key} » (${where})`)
    failed = true
  }
}
for (const key of fr.keys()) {
  if (!used.has(key)) {
    console.error(`fr: unused « ${key} »`)
    failed = true
  }
}
if (failed) process.exit(1)
console.log(`i18n: ${used.size} texts, all translated to French`)
