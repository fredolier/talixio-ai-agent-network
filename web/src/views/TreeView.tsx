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
import { count, flat, moment, statusLabel, targetLabel } from '@/lib/format'
import { t, tn } from '@/lib/i18n'
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
        eyebrow={t('Cluster')}
        title={t('The agents, live')}
        lead={t('{agents} on {machines}. The master runs on {host}.', {
          agents: tn(all.length, '{n} agent', '{n} agents'),
          machines: tn(machines, '{n} machine', '{n} machines'),
          host: cluster.tree.host,
        })}
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
        <ul className="flex flex-col" aria-label={t('Agents')}>
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
              <Empty title={t('The master is alone')}>
                {t('Ask it, in the conversation, to create an agent: it will appear here with the permissions the master granted it.')}
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
            <SheetDescription>{t('Agent details')}</SheetDescription>
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
      {/* the rail of the tree: one indent per level, an elbow towards the agent */}
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
          {agent.depth === 0 && <Badge>{t('master')}</Badge>}
          {agent.unverified && (
            <Badge variant="outline" className="border-st-off/50 text-st-off" title={t('What this agent declares exceeds what its parent can grant. Its actual permissions remain those its parent enforces.')}>
              {t('inconsistent declaration')}
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
  const list = (items?: string[]) => (items?.length ? items.join(', ') : <span className="text-muted-foreground">{t('none')}</span>)
  return (
    <div className="flex flex-col gap-5">
      <div>
        <p className="eyebrow">{agent.depth === 0 ? t('Master agent') : t('Level {depth}', { depth: agent.depth })}</p>
        <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1">
          <h2 className="text-lg font-semibold tracking-tight">{agent.name}</h2>
          <StatusMark status={agent.status} />
        </div>
        <p className="mt-0.5 text-muted-foreground">{agent.role}</p>
      </div>

      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2.5">
        <Field label={t('Model')} mono>{agent.model}</Field>
        <Field label={t('Machine')}>{agent.host} <span className="text-muted-foreground">({agent.os})</span></Field>
        <Field label={t('Addresses')} mono>{agent.ips.join(', ') || '—'}</Field>
        <Field label={t('Deployed')}>{targetLabel(agent.target)}</Field>
        <Field label={t('Tokens')} mono>
          {count(agent.tokens_used)}
          {caps.max_tokens ? ` / ${count(caps.max_tokens)}` : ` ${t('(no cap)')}`}
        </Field>
        <Field label={t('Since')}>{moment(agent.started_at_ms)}</Field>
        <Field label={t('Identifier')} mono>{agent.id}</Field>
      </dl>

      <div className="border-t pt-4">
        <p className="eyebrow">{t('What it is allowed to do')}</p>
        <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2.5">
          <Field label={t('Read')} mono>{list(caps.fs_read)}</Field>
          <Field label={t('Write')} mono>{list(caps.fs_write)}</Field>
          <Field label={t('Models')} mono>{list(caps.models)}</Field>
          <Field label={t('Service calls')}>
            {Object.keys(caps.max_calls ?? {}).length ? (
              <ul className="flex flex-col gap-1">
                {Object.entries(caps.max_calls ?? {}).map(([nature, max]) => (
                  <li key={nature} className="num">
                    {t('{nature}: {used} / {max}', { nature, used: count(pick(agent.calls_used ?? {}, nature) ?? 0), max: count(max) })}
                  </li>
                ))}
              </ul>
            ) : (
              <span className="text-muted-foreground">{t('none')}</span>
            )}
          </Field>
          <Field label={t('Children')}>
            {caps.spawn
              ? t('{children} at most, over {levels} ({targets})', {
                  children: caps.spawn.max_children,
                  levels: tn(caps.spawn.max_depth, '{n} level', '{n} levels'),
                  targets: caps.spawn.targets.map(targetLabel).join(', '),
                })
              : <span className="text-muted-foreground">{t('cannot create any')}</span>}
          </Field>
          <Field label={t('Commands')}>
            {caps.exec?.length ? (
              <ul className="flex flex-col gap-1.5">
                {caps.exec.map((rule, index) => (
                  <li key={index}><RuleLine rule={rule} /></li>
                ))}
              </ul>
            ) : (
              <span className="text-muted-foreground">{t('no command')}</span>
            )}
          </Field>
        </dl>
      </div>

      {agent.depth > 0 && agent.status !== 'stopped' && (
        <div className="border-t pt-4">
          <Button variant="destructive" onClick={() => setConfirm(true)}>
            {t('Stop this agent and its descendants')}
          </Button>
          {error && <p className="mt-2 text-destructive" role="alert">{error}</p>}
        </div>
      )}

      <AlertDialog open={confirm} onOpenChange={setConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('Stop “{name}”?', { name: agent.name })}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('The agent and all its descendants stop. Whatever was deployed on another machine is removed from it. Current status: {status}.', { status: statusLabel(agent.status) })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('Cancel')}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                setConfirm(false)
                setError(null)
                cluster.stop(agent.id).catch((e: Error) => setError(e.message))
              }}
            >
              {t('Stop')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
