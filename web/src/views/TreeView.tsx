import { useState } from 'react'
import { Empty, Field, PageHeader, RuleLine, StatusMark } from '@/components/bits'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { count, flat, moment, plural, STATUS, targetLabel } from '@/lib/format'
import type { Cluster } from '@/lib/store'
import type { Agent, Status } from '@/lib/types'
import { cn, pick } from '@/lib/utils'

export function TreeView({ cluster }: { cluster: Cluster }) {
  const all = flat(cluster.tree)
  const [selectedId, setSelectedId] = useState(cluster.tree.id)
  const [sheet, setSheet] = useState(false)
  const selected = all.find((a) => a.id === selectedId) ?? cluster.tree
  const machines = new Set(all.map((a) => a.host)).size
  const tally = new Map<Status, number>()
  all.forEach((a) => tally.set(a.status, (tally.get(a.status) ?? 0) + 1))

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow="Grappe"
        title="Les agents, en direct"
        lead={`${plural(all.length, 'agent')} sur ${plural(machines, 'machine')}. Le maître tourne sur ${cluster.tree.host}.`}
      >
        <div className="flex flex-wrap gap-x-4 gap-y-1">
          {[...tally].map(([status, n]) => (
            <span key={status} className="inline-flex items-baseline gap-1.5">
              <span className="num text-base font-medium">{n}</span>
              <StatusMark status={status} />
            </span>
          ))}
        </div>
      </PageHeader>

      <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(320px,400px)]">
        <ul className="flex flex-col" aria-label="Agents">
          {all.map((agent) => (
            <AgentRow
              key={agent.id}
              agent={agent}
              selected={agent.id === selected.id}
              onSelect={() => {
                setSelectedId(agent.id)
                if (window.matchMedia('(max-width: 1023px)').matches) setSheet(true)
              }}
            />
          ))}
          {all.length === 1 && (
            <li className="mt-4">
              <Empty title="Le maître est seul">
                Demandez-lui, dans le dialogue, de créer un agent : il apparaîtra ici avec les droits qu'il lui aura accordés.
              </Empty>
            </li>
          )}
        </ul>
        <aside className="sticky top-6 hidden rounded-lg border bg-card p-5 lg:block" aria-live="polite">
          <AgentDetail agent={selected} cluster={cluster} />
        </aside>
      </div>

      <Sheet open={sheet} onOpenChange={setSheet}>
        <SheetContent side="bottom" className="max-h-[85dvh] overflow-y-auto p-5 lg:hidden">
          <SheetHeader className="sr-only">
            <SheetTitle>{selected.name}</SheetTitle>
            <SheetDescription>Fiche de l'agent</SheetDescription>
          </SheetHeader>
          <AgentDetail agent={selected} cluster={cluster} />
        </SheetContent>
      </Sheet>
    </div>
  )
}

function AgentRow({ agent, selected, onSelect }: { agent: Agent; selected: boolean; onSelect: () => void }) {
  const limit = agent.capabilities.max_tokens
  const depth = Math.min(agent.depth, 6)
  return (
    <li className="relative flex">
      {/* le rail de l'arbre : un retrait par niveau, un coude vers l'agent */}
      {depth > 0 && (
        <span className="flex shrink-0 justify-end" style={{ width: depth * 28 }} aria-hidden>
          <span className="mr-1.5 block h-1/2 w-4 rounded-bl-md border-b border-l border-rail" />
        </span>
      )}
      <button
        type="button"
        onClick={onSelect}
        aria-current={selected}
        className={cn(
          'group my-1 flex min-w-0 flex-1 flex-col gap-1.5 rounded-lg border bg-card px-4 py-3 text-left transition-colors outline-none hover:border-foreground/30 focus-visible:ring-3 focus-visible:ring-ring/50',
          selected && 'border-foreground bg-accent/60',
        )}
      >
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span className="font-semibold">{agent.name}</span>
          <StatusMark status={agent.status} />
          {agent.depth === 0 && <Badge>maître</Badge>}
          {agent.unverified && (
            <Badge variant="outline" className="border-st-off/50 text-st-off" title="Ce que cet agent déclare dépasse ce que son parent peut accorder. Ses droits réels restent ceux que son parent lui applique.">
              déclaration incohérente
            </Badge>
          )}
          <span className="num ml-auto text-xs text-muted-foreground">{agent.model}</span>
        </span>
        <span className="truncate text-[13px] text-muted-foreground">
          {agent.role} · {agent.host}
          {agent.depth > 0 && ` · ${targetLabel(agent.target)}`}
        </span>
        {limit ? (
          <span className="mt-0.5 flex items-center gap-2">
            <span className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
              <span className="block h-full rounded-full bg-foreground" style={{ width: `${Math.min(100, (agent.tokens_used / limit) * 100)}%` }} />
            </span>
            <span className="num text-[11px] text-muted-foreground">
              {count(agent.tokens_used)} / {count(limit)}
            </span>
          </span>
        ) : null}
      </button>
    </li>
  )
}

function AgentDetail({ agent, cluster }: { agent: Agent; cluster: Cluster }) {
  const caps = agent.capabilities
  const [confirm, setConfirm] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const list = (items?: string[]) => (items?.length ? items.join(', ') : <span className="text-muted-foreground">aucun</span>)
  return (
    <div className="flex flex-col gap-5">
      <div>
        <p className="eyebrow">{agent.depth === 0 ? 'Agent maître' : `Niveau ${agent.depth}`}</p>
        <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1">
          <h2 className="text-lg font-semibold tracking-tight">{agent.name}</h2>
          <StatusMark status={agent.status} />
        </div>
        <p className="mt-0.5 text-muted-foreground">{agent.role}</p>
      </div>

      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2.5">
        <Field label="Modèle" mono>{agent.model}</Field>
        <Field label="Machine">{agent.host} <span className="text-muted-foreground">({agent.os})</span></Field>
        <Field label="Adresses" mono>{agent.ips.join(', ') || '—'}</Field>
        <Field label="Déployé">{targetLabel(agent.target)}</Field>
        <Field label="Tokens" mono>
          {count(agent.tokens_used)}
          {caps.max_tokens ? ` / ${count(caps.max_tokens)}` : ' (sans plafond)'}
        </Field>
        <Field label="Depuis">{moment(agent.started_at_ms)}</Field>
        <Field label="Identifiant" mono>{agent.id}</Field>
      </dl>

      <div className="border-t pt-4">
        <p className="eyebrow">Ce qu'il a le droit de faire</p>
        <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2.5">
          <Field label="Lire" mono>{list(caps.fs_read)}</Field>
          <Field label="Écrire" mono>{list(caps.fs_write)}</Field>
          <Field label="Modèles" mono>{list(caps.models)}</Field>
          <Field label="Appels de service">
            {Object.keys(caps.max_calls ?? {}).length ? (
              <ul className="flex flex-col gap-1">
                {Object.entries(caps.max_calls ?? {}).map(([nature, max]) => (
                  <li key={nature} className="num">
                    {nature} : {count(pick(agent.calls_used ?? {}, nature) ?? 0)} / {count(max)}
                  </li>
                ))}
              </ul>
            ) : (
              <span className="text-muted-foreground">aucun</span>
            )}
          </Field>
          <Field label="Enfants">
            {caps.spawn
              ? `${caps.spawn.max_children} au plus, sur ${caps.spawn.max_depth} niveau${caps.spawn.max_depth > 1 ? 'x' : ''} (${caps.spawn.targets.map(targetLabel).join(', ')})`
              : <span className="text-muted-foreground">ne peut pas en créer</span>}
          </Field>
          <Field label="Commandes">
            {caps.exec?.length ? (
              <ul className="flex flex-col gap-1.5">
                {caps.exec.map((rule, index) => (
                  <li key={index}><RuleLine rule={rule} /></li>
                ))}
              </ul>
            ) : (
              <span className="text-muted-foreground">aucune</span>
            )}
          </Field>
        </dl>
      </div>

      {agent.depth > 0 && agent.status !== 'stopped' && (
        <div className="border-t pt-4">
          <Button variant="destructive" onClick={() => setConfirm(true)}>
            Arrêter cet agent et sa descendance
          </Button>
          {error && <p className="mt-2 text-destructive" role="alert">{error}</p>}
        </div>
      )}

      <AlertDialog open={confirm} onOpenChange={setConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Arrêter « {agent.name} » ?</AlertDialogTitle>
            <AlertDialogDescription>
              L'agent et toute sa descendance s'arrêtent. Ce qui a été déployé sur une autre machine en est retiré. État actuel : {pick(STATUS, agent.status) ?? agent.status}.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Annuler</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                setConfirm(false)
                setError(null)
                cluster.stop(agent.id).catch((e: Error) => setError(e.message))
              }}
            >
              Arrêter
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
