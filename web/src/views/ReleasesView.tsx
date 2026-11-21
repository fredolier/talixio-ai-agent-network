import { PageHeader, Rich } from '@/components/bits'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import { day } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import { cn } from '@/lib/utils'

const TONE: Record<string, string> = {
  sécurité: 'text-st-working',
  attention: 'text-st-wait',
  correction: 'text-foreground',
}

export function ReleasesView({ cluster }: { cluster: Cluster }) {
  const data = cluster.releases
  return (
    <div className="flex flex-col gap-6">
      <PageHeader eyebrow="Nouveautés" title="Ce qui a changé" lead={data ? `Version après version. Le maître tourne en v${data.version}.` : undefined} />
      {!data ? (
        <Skeleton className="h-60 max-w-3xl rounded-lg" />
      ) : (
        <ol className="max-w-3xl">
          {data.releases.map((release, index) => {
            const current = release.version === data.version
            return (
              <li key={release.version} className="grid grid-cols-[1.25rem_minmax(0,1fr)] gap-x-4">
                <span className="flex flex-col items-center" aria-hidden>
                  <span className={cn('mt-6 size-2.5 rounded-[3px] border border-foreground', current && 'bg-signal')} />
                  {index < data.releases.length - 1 && <span className="w-px flex-1 bg-rail" />}
                </span>
                <article className={cn('mb-5 rounded-lg border bg-card p-5', current && 'border-foreground')}>
                  <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                    <span className="num text-xs text-muted-foreground">v{release.version}</span>
                    <h2 className="text-base font-semibold">{release.title}</h2>
                    {current && <Badge>installée</Badge>}
                    <time className="ml-auto text-xs text-muted-foreground">{day(release.date)}</time>
                  </div>
                  <p className="mt-2 text-muted-foreground"><Rich text={release.summary} /></p>
                  <ul className="mt-4 flex flex-col gap-2.5">
                    {release.changes.map((change, i) => (
                      <li key={i} className={cn('grid grid-cols-[6.5rem_minmax(0,1fr)] gap-x-3', change.kind === 'attention' && '-mx-2 rounded-md bg-st-wait/10 px-2 py-2')}>
                        <span className={cn('eyebrow pt-1', TONE[change.kind])}>{change.kind === 'attention' ? 'à savoir' : change.kind}</span>
                        <span><Rich text={change.text} /></span>
                      </li>
                    ))}
                  </ul>
                </article>
              </li>
            )
          })}
        </ol>
      )}
    </div>
  )
}
