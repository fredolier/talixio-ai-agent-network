// Interface languages. English is the source text, written directly in the code: `t('Cluster')`.
// Every other language is a table from the English text to its translation
// (`src/locales/<lang>/*.json`); a missing entry falls back to English.
// `npm run i18n` checks that every text used in the code has a French translation.

import { useSyncExternalStore } from 'react'

export const LANGS = [
  { id: 'en', label: 'English' },
  { id: 'fr', label: 'Français' },
] as const
export type Lang = (typeof LANGS)[number]['id']

const KEY = 'talixio-lang'

const tables: Record<Lang, Record<string, string>> = {
  en: {},
  fr: Object.assign({}, ...Object.values(import.meta.glob<Record<string, string>>('../locales/fr/*.json', { eager: true, import: 'default' }))),
}

const known = (value: unknown): value is Lang => LANGS.some((l) => l.id === value)

/** The saved choice, else the first browser language we speak, else English. */
function initial(): Lang {
  const saved = localStorage.getItem(KEY)
  if (known(saved)) return saved
  for (const tag of navigator.languages ?? []) {
    const short = tag.slice(0, 2).toLowerCase()
    if (known(short)) return short
  }
  return 'en'
}

let lang: Lang = initial()
const listeners = new Set<() => void>()
document.documentElement.lang = lang

export const currentLang = () => lang

export function setLang(next: Lang) {
  if (next === lang) return
  lang = next
  localStorage.setItem(KEY, next)
  document.documentElement.lang = next
  listeners.forEach((listener) => listener())
}

/** The current language; the component is rendered again when it changes. */
export function useLang(): Lang {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    () => lang,
  )
}

type Vars = Record<string, string | number>

const fill = (text: string, vars?: Vars) =>
  vars ? text.replace(/\{(\w+)\}/g, (whole, name: string) => (Object.hasOwn(vars, name) ? String(vars[name]) : whole)) : text

const lookup = (text: string) => (Object.hasOwn(tables[lang], text) ? tables[lang][text] : text)

/** Translates an English text; `{name}` is replaced by `vars.name`. Call it while rendering, not at module load. */
export const t = (text: string, vars?: Vars) => fill(lookup(text), vars)

/** Same, with a singular and a plural form chosen by the rules of the current language; `{n}` is the formatted number. */
export function tn(n: number, one: string, other: string, vars?: Vars) {
  const form = new Intl.PluralRules(lang).select(n) === 'one' ? one : other
  return fill(lookup(form), { n: n.toLocaleString(lang), ...vars })
}

/** Marks a text kept in a table and translated later with `t(entry)`: lets `npm run i18n` see it. */
export const tx = (text: string) => text
