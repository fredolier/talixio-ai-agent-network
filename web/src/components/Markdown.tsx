import type { ReactNode } from 'react'

// Les réponses des modèles sont écrites en Markdown. On en rend l'essentiel (titres, listes,
// gras, italique, code) en éléments React : le texte reste du texte, aucun HTML venu du modèle
// n'est interprété, aucun lien n'est rendu cliquable.

const INLINE = /(`[^`\n]+`|\*\*[^*\n]+\*\*|__[^_\n]+__|\*[^*\s][^*\n]*\*)/

function inline(text: string): ReactNode[] {
  return text.split(INLINE).map((part, i) => {
    if (part.length > 2 && part.startsWith('`') && part.endsWith('`')) {
      return <code key={i} className="num rounded bg-muted px-1 py-0.5 text-[0.92em]">{part.slice(1, -1)}</code>
    }
    if (part.length > 4 && ((part.startsWith('**') && part.endsWith('**')) || (part.startsWith('__') && part.endsWith('__')))) {
      return <strong key={i} className="font-semibold">{inline(part.slice(2, -2))}</strong>
    }
    if (part.length > 2 && part.startsWith('*') && part.endsWith('*')) {
      return <em key={i}>{inline(part.slice(1, -1))}</em>
    }
    return part
  })
}

type Item = { text: string; children: ReactNode[] }

export function Markdown({ text }: { text: string }) {
  const lines = text.replace(/\r\n?/g, '\n').split('\n')
  const out: ReactNode[] = []
  let i = 0
  const key = () => out.length
  while (i < lines.length) {
    const line = lines[i]
    if (!line.trim()) {
      i++
      continue
    }
    // bloc de code
    const fence = line.match(/^\s*(```|~~~)/)
    if (fence) {
      const body: string[] = []
      i++
      while (i < lines.length && !lines[i].trimStart().startsWith(fence[1])) body.push(lines[i++])
      i++
      out.push(
        <pre key={key()} className="num overflow-x-auto rounded-md bg-muted p-3 text-[12.5px] leading-relaxed">
          <code>{body.join('\n')}</code>
        </pre>,
      )
      continue
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.*)$/)
    if (heading) {
      const big = heading[1].length <= 2
      out.push(
        <p key={key()} className={big ? 'mt-1 text-[1.05em] font-semibold' : 'font-semibold'}>
          {inline(heading[2].replace(/\s+#+\s*$/, ''))}
        </p>,
      )
      i++
      continue
    }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      out.push(<hr key={key()} className="border-border" />)
      i++
      continue
    }
    // liste (à puces ou numérotée), avec les lignes de suite indentées
    const bullet = /^\s*([-*+]|\d+[.)])\s+(.*)$/
    const first = line.match(bullet)
    if (first) {
      const ordered = /\d/.test(first[1])
      const items: Item[] = []
      while (i < lines.length) {
        const match = lines[i].match(bullet)
        if (match && /\d/.test(match[1]) === ordered) {
          items.push({ text: match[2], children: [] })
        } else if (lines[i].trim() && /^\s+/.test(lines[i]) && items.length) {
          items[items.length - 1].text += ` ${lines[i].trim()}`
        } else {
          break
        }
        i++
      }
      const List = ordered ? 'ol' : 'ul'
      out.push(
        <List key={key()} className={ordered ? 'list-decimal space-y-1 pl-5' : 'list-disc space-y-1 pl-5'}>
          {items.map((item, n) => (
            <li key={n}>{inline(item.text)}</li>
          ))}
        </List>,
      )
      continue
    }
    if (/^\s*>/.test(line)) {
      const quote: string[] = []
      while (i < lines.length && /^\s*>/.test(lines[i])) quote.push(lines[i++].replace(/^\s*>\s?/, ''))
      out.push(
        <blockquote key={key()} className="border-l-2 pl-3 text-muted-foreground whitespace-pre-wrap">
          {inline(quote.join('\n'))}
        </blockquote>,
      )
      continue
    }
    // paragraphe : jusqu'à la ligne vide ou au bloc suivant
    const paragraph: string[] = []
    while (i < lines.length && lines[i].trim() && !/^\s*(```|~~~|#{1,6}\s|>)/.test(lines[i]) && !(paragraph.length && bullet.test(lines[i]))) {
      paragraph.push(lines[i++])
    }
    // quoi qu'il arrive, on avance : une ligne qu'aucun bloc ne prend est un paragraphe
    if (!paragraph.length) paragraph.push(lines[i++])
    out.push(
      <p key={key()} className="whitespace-pre-wrap">
        {inline(paragraph.join('\n'))}
      </p>,
    )
  }
  return <div className="flex flex-col gap-2.5 [overflow-wrap:anywhere]">{out}</div>
}
