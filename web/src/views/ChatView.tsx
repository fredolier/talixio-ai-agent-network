import { useEffect, useRef, useState } from 'react'
import { ArrowUp, LoaderCircle } from 'lucide-react'
import { Markdown } from '@/components/Markdown'
import { Empty, PageHeader } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { hour } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import { cn } from '@/lib/utils'

const MINE = "de l'utilisateur : "
// une tâche planifiée : votre consigne, écrite d'avance
const TASK = /^de la tâche planifiée « ([^»]*) » : /

export function ChatView({ cluster }: { cluster: Cluster }) {
  const [text, setText] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [sending, setSending] = useState(false)
  const end = useRef<HTMLDivElement>(null)
  const lines = cluster.events.filter(
    (e) =>
      e.agent_id === cluster.tree.id &&
      (e.kind === 'message' ||
        (e.kind === 'input' && (e.text.startsWith(MINE) || TASK.test(e.text))) ||
        // une erreur du maître se dit ici aussi : sans cela, une question resterait sans réponse ni raison.
        // Celles des appels de ses enfants restent au journal.
        (e.kind === 'error' && (!e.text.startsWith('appel du modèle') || e.text.includes(cluster.tree.id)))),
  )
  const last = lines[lines.length - 1]
  // il travaille : son état le dit, ou il vient de recevoir une question restée sans réponse
  const waiting = cluster.tree.status === 'working' || (last?.kind === 'input' && Date.now() - last.ts_ms < 20 * 60 * 1000)
  useEffect(() => {
    // pas de valeur de retour : selon le navigateur, scrollIntoView renvoie une promesse
    void end.current?.scrollIntoView({ block: 'end' })
  }, [lines.length, waiting])

  const send = async () => {
    const message = text.trim()
    if (!message || sending) return
    setSending(true)
    setError(null)
    try {
      await cluster.send(message)
      setText('')
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setSending(false)
    }
  }

  return (
    <div className="flex min-h-full flex-col gap-6">
      <PageHeader eyebrow="Dialogue" title="Parler au maître" lead="Vos consignes et ses réponses. Ce qu'il délègue se suit dans la grappe et le journal." />
      <div className="flex flex-1 flex-col gap-3" aria-live="polite">
        {lines.length === 0 && <Empty title="Rien encore">Écrivez une consigne : le maître décidera s'il la traite seul ou s'il crée des agents.</Empty>}
        {lines.map((event) => {
          const mine = event.kind === 'input'
          const failed = event.kind === 'error'
          const task = mine ? event.text.match(TASK) : null
          return (
            <div key={event.id} className={cn('flex max-w-[min(680px,90%)] flex-col gap-1', mine && 'self-end items-end')}>
              <span className="eyebrow">{mine ? (task ? `tâche planifiée · ${task[1]}` : 'vous') : cluster.tree.name} · {hour(event.ts_ms)}</span>
              <div
                className={cn('rounded-lg border px-3.5 py-2.5', mine ? 'border-primary bg-primary text-primary-foreground' : failed ? 'border-destructive/40 bg-destructive/5 text-destructive' : 'bg-card')}
                role={failed ? 'alert' : undefined}
              >
                {mine || failed ? <p className="whitespace-pre-wrap [overflow-wrap:anywhere]">{mine ? event.text.slice(task ? task[0].length : MINE.length) : event.text}</p> : <Markdown text={event.text} />}
              </div>
            </div>
          )
        })}
        {waiting && (
          <p className="eyebrow flex items-center gap-2" role="status">
            <LoaderCircle className="size-3.5 animate-spin" aria-hidden />
            {cluster.tree.name} travaille…
          </p>
        )}
        <div ref={end} />
      </div>
      <form
        className="sticky bottom-4 flex items-end gap-2 rounded-lg border bg-card p-2 shadow-sm"
        onSubmit={(e) => {
          e.preventDefault()
          send()
        }}
      >
        <Textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
          rows={2}
          placeholder="Écrire au maître…"
          aria-label="Message au maître"
          className="min-h-0 resize-none border-0 bg-transparent shadow-none focus-visible:ring-0 dark:bg-transparent"
        />
        <Button type="submit" size="icon-lg" disabled={!text.trim() || sending} aria-label="Envoyer">
          <ArrowUp />
        </Button>
      </form>
      {error && <p className="text-destructive" role="alert">{error}</p>}
    </div>
  )
}
