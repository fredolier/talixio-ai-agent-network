import { useMemo, useState } from 'react'
import { Empty, Field, PageHeader, RuleLine, StatusMark } from '@/components/bits'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { flat, moment, plural, targetLabel } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import type { RoleEntry } from '@/lib/types'

export function RolesView({ cluster }: { cluster: Cluster }) {
  const [filter, setFilter] = useState('')
  const alive = useMemo(() => new Map(flat(cluster.tree).map((a) => [a.id, a])), [cluster.tree])
  const groups = useMemo(() => {
    const wanted = filter.trim().toLowerCase()
    const byRole = new Map<string, RoleEntry[]>()
    for (const entry of cluster.roles ?? []) {
      const haystack = `${entry.role} ${entry.name} ${entry.instructions ?? ''} ${entry.objective ?? ''}`.toLowerCase()
      if (wanted && !haystack.includes(wanted)) continue
      byRole.set(entry.role, [...(byRole.get(entry.role) ?? []), entry])
    }
    return [...byRole]
  }, [cluster.roles, filter])

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow="Maîtrise · Rôles"
        title="Les rôles créés dans la grappe"
        lead="Chaque agent créé, par le maître ou plus bas, est inscrit ici avec ce qui lui a été accordé. Le répertoire survit aux redémarrages."
      >
        <Input type="search" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Chercher un rôle, un nom, une consigne…" aria-label="Chercher un rôle" className="w-full sm:w-72" />
      </PageHeader>
      {!cluster.roles ? (
        <Skeleton className="h-40 rounded-lg" />
      ) : groups.length === 0 ? (
        <Empty title={filter ? 'Aucun rôle ne correspond' : 'Aucun rôle encore'}>
          {!filter && 'Dès qu’un agent en crée un autre, son rôle et ses droits apparaissent ici.'}
        </Empty>
      ) : (
        groups.map(([role, entries]) => (
          <section key={role} className="rounded-lg border bg-card">
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-5 py-3.5">
              <h2 className="text-base font-semibold">{role || 'sans rôle'}</h2>
              <span className="text-xs text-muted-foreground">{plural(entries.length, 'agent')}</span>
            </div>
            <ul>
              {entries.map((entry) => {
                const live = alive.get(entry.agent_id)
                const caps = entry.capabilities
                return (
                  <li key={entry.agent_id} className="border-t px-5 py-4">
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                      <span className="font-medium">{entry.name}</span>
                      {entry.unverified && <span className="text-xs text-st-off" title="Ce que cet agent déclare dépasse ce que son parent peut accorder.">déclaration incohérente</span>}
                      {live ? <StatusMark status={live.status} /> : <span className="text-xs text-muted-foreground">n'est plus dans la grappe</span>}
                      <span className="num ml-auto text-xs text-muted-foreground">{moment(entry.first_seen_ms)}</span>
                    </div>
                    <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2">
                      <Field label="Créé par">{entry.parent_id ? (alive.get(entry.parent_id)?.name ?? entry.parent_id) : '—'} · niveau {entry.depth} · {targetLabel(entry.target) || '—'}</Field>
                      <Field label="Modèle" mono>{entry.model || '—'}</Field>
                      {entry.objective != null ? (
                        <>
                          <Field label="Mission"><span className="whitespace-pre-wrap">{entry.objective}</span></Field>
                          <Field label="Réussite"><span className="whitespace-pre-wrap">{entry.criteria}</span></Field>
                          <Field label="Consignes"><span className="whitespace-pre-wrap">{entry.instructions}</span></Field>
                        </>
                      ) : (
                        <Field label="Consignes"><span className="text-muted-foreground">données par son parent, pas encore remontées jusqu'au maître</span></Field>
                      )}
                      <Field label="Droits">
                        <span className="text-muted-foreground">
                          lit {caps.fs_read?.length ?? 0} dossier{(caps.fs_read?.length ?? 0) > 1 ? 's' : ''}, écrit dans {caps.fs_write?.length ?? 0}
                          {caps.spawn ? `, peut créer ${caps.spawn.max_children} enfant${caps.spawn.max_children > 1 ? 's' : ''}` : ', sans enfant'}
                        </span>
                        {!!caps.exec?.length && (
                          <ul className="mt-1.5 flex flex-col gap-1">
                            {caps.exec.map((rule, index) => <li key={index}><RuleLine rule={rule} /></li>)}
                          </ul>
                        )}
                      </Field>
                    </dl>
                  </li>
                )
              })}
            </ul>
          </section>
        ))
      )}
    </div>
  )
}
