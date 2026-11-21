import { useMemo, useState } from 'react'
import { Empty, PageHeader } from '@/components/bits'
import { Input } from '@/components/ui/input'
import { hour, KIND } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import { cn } from '@/lib/utils'

const TONE: Record<string, string> = {
  tool_error: 'text-st-off',
  error: 'text-st-off',
  spawn: 'text-st-working',
  link: 'text-st-wait',
  message: 'text-foreground',
  report: 'text-foreground',
}
const SHOWN = 800

export function JournalView({ cluster }: { cluster: Cluster }) {
  const [filter, setFilter] = useState('')
  const rows = useMemo(() => {
    const wanted = filter.trim().toLowerCase()
    const matching = wanted
      ? cluster.events.filter((e) => `${e.agent_name} ${KIND[e.kind] ?? e.kind} ${e.text}`.toLowerCase().includes(wanted))
      : cluster.events
    return matching.slice(-SHOWN).reverse()
  }, [cluster.events, filter])

  return (
    <div className="flex flex-col gap-6">
      <PageHeader eyebrow="Journal" title="Tout ce qui remonte" lead="Chaque geste de chaque agent, du plus récent au plus ancien.">
        <Input
          type="search"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filtrer : agent, genre, texte…"
          aria-label="Filtrer le journal"
          className="w-full sm:w-72"
        />
      </PageHeader>
      {rows.length === 0 ? (
        <Empty title={filter ? 'Rien ne correspond' : 'Journal vide'} />
      ) : (
        <ol className="overflow-hidden rounded-lg border bg-card">
          {rows.map((event) => (
            <li
              key={event.id}
              className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-x-3 gap-y-0.5 border-b px-4 py-2 last:border-b-0 hover:bg-muted/60 md:grid-cols-[4.5rem_8rem_5rem_minmax(0,1fr)] md:items-baseline"
            >
              <time className="num text-xs text-muted-foreground">{hour(event.ts_ms)}</time>
              <span className="truncate text-[13px] font-medium">{event.agent_name}</span>
              <span className={cn('eyebrow', TONE[event.kind])}>{KIND[event.kind] ?? event.kind}</span>
              <span className="col-start-2 whitespace-pre-wrap [overflow-wrap:anywhere] md:col-start-4">{event.text}</span>
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}
