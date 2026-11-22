import { useMemo, useState } from 'react'
import { Empty, Field, PageHeader, RuleLine, StatusMark } from '@/components/bits'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { flat, moment, targetLabel } from '@/lib/format'
import { t, tn } from '@/lib/i18n'
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
  const installed = useMemo(() => {
    const wanted = filter.trim().toLowerCase()
    return cluster.installedRoles.filter((role) => !wanted || `${role.name} ${role.description} ${role.instructions}`.toLowerCase().includes(wanted))
  }, [cluster.installedRoles, filter])
  const made = (name: string) => (cluster.roles ?? []).filter((entry) => entry.role === name).length
  const [packsBefore, packsAfter] = t('From packs placed on the machine by {command}. The master can hand them out, not change them: instructions and commands are verified by hash.').split('{command}')

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow={t('Control · Roles')}
        title={t('The roles of the cluster')}
        lead={t("The roles installed by the operator, which the master can give to an agent, then every agent created, by the master or further down, with what it was granted. The directory survives restarts.")}
      >
        <Input type="search" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t('Search for a role, a name, an instruction…')} aria-label={t('Search for a role')} className="w-full sm:w-72" />
      </PageHeader>
      {installed.length > 0 && (
        <section aria-labelledby="roles-installes" className="flex flex-col gap-3">
          <div>
            <h2 id="roles-installes" className="text-base font-semibold">{t('Installed roles')}</h2>
            <p className="mt-0.5 max-w-2xl text-sm text-muted-foreground">
              {packsBefore}<span className="font-mono text-[12.5px]">talixio-agent pack install</span>{packsAfter}
            </p>
          </div>
          <ul className="grid items-start gap-3 lg:grid-cols-2">
            {installed.map((role) => (
              <li key={role.name} className="rounded-lg border bg-card px-5 py-4">
                <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                  <h3 className="font-mono text-sm font-semibold">{role.name}</h3>
                  <span className="ml-auto text-xs text-muted-foreground">{made(role.name) ? tn(made(role.name), '{n} agent created', '{n} agents created') : t('not used yet')}</span>
                </div>
                <p className="mt-1.5">{role.description}</p>
                <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2">
                  <Field label={t('Pack')}>
                    {role.pack} · {t('version {version}', { version: role.version })} · <span className="font-mono text-[12.5px]" title={t('Pack identifier (hash of its content): {id}', { id: role.pack_id })}>{role.pack_id.slice(0, 12)}</span>
                  </Field>
                  {(role.model || role.target) && (
                    <Field label={t('Recommended')}>{[role.model && t('model {model}', { model: role.model }), role.target && t('target {target}', { target: role.target })].filter(Boolean).join(' · ')}</Field>
                  )}
                  <Field label={t('Commands')}>
                    {role.exec.length ? (
                      <ul className="flex flex-col gap-1">
                        {role.exec.map((rule, index) => (
                          <li key={index} title={rule.program}><RuleLine rule={{ ...rule, program: rule.program.split('/').pop() ?? rule.program }} /></li>
                        ))}
                      </ul>
                    ) : (
                      <span className="text-muted-foreground">{t('no command')}</span>
                    )}
                  </Field>
                  <Field label={t('Instructions')}>
                    <details>
                      <summary className="cursor-pointer text-muted-foreground">{t('Read the instructions given to the agent')}</summary>
                      <p className="mt-2 whitespace-pre-wrap">{role.instructions}</p>
                    </details>
                  </Field>
                </dl>
              </li>
            ))}
          </ul>
        </section>
      )}
      {installed.length > 0 && <h2 className="text-base font-semibold">{t('Agents created')}</h2>}
      {!cluster.roles ? (
        <Skeleton className="h-40 rounded-lg" />
      ) : groups.length === 0 ? (
        <Empty title={filter ? t('No role matches') : t('No role yet')}>
          {!filter && t('As soon as an agent creates another, its role and permissions appear here.')}
        </Empty>
      ) : (
        groups.map(([role, entries]) => (
          <section key={role} className="rounded-lg border bg-card">
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-5 py-3.5">
              <h2 className="text-base font-semibold">{role || t('no role')}</h2>
              <span className="text-xs text-muted-foreground">{tn(entries.length, '{n} agent', '{n} agents')}</span>
            </div>
            <ul>
              {entries.map((entry) => {
                const live = alive.get(entry.agent_id)
                const caps = entry.capabilities
                return (
                  <li key={entry.agent_id} className="border-t px-5 py-4">
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                      <span className="font-medium">{entry.name}</span>
                      {entry.unverified && <span className="text-xs text-st-off" title={t('What this agent declares exceeds what its parent can grant.')}>{t('inconsistent declaration')}</span>}
                      {live ? <StatusMark status={live.status} /> : <span className="text-xs text-muted-foreground">{t('no longer in the cluster')}</span>}
                      <span className="num ml-auto text-xs text-muted-foreground">{moment(entry.first_seen_ms)}</span>
                    </div>
                    <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2">
                      <Field label={t('Created by')}>{entry.parent_id ? (alive.get(entry.parent_id)?.name ?? entry.parent_id) : '—'} · {t('level {depth}', { depth: entry.depth })} · {targetLabel(entry.target) || '—'}</Field>
                      <Field label={t('Model')} mono>{entry.model || '—'}</Field>
                      {entry.objective != null ? (
                        <>
                          <Field label={t('Mission')}><span className="whitespace-pre-wrap">{entry.objective}</span></Field>
                          <Field label={t('Success criteria')}><span className="whitespace-pre-wrap">{entry.criteria}</span></Field>
                          <Field label={t('Instructions')}><span className="whitespace-pre-wrap">{entry.instructions}</span></Field>
                        </>
                      ) : (
                        <Field label={t('Instructions')}><span className="text-muted-foreground">{t('given by its parent, not yet reported up to the master')}</span></Field>
                      )}
                      <Field label={t('Permissions')}>
                        <span className="text-muted-foreground">
                          {[
                            tn(caps.fs_read?.length ?? 0, 'reads {n} folder', 'reads {n} folders'),
                            t('writes to {count}', { count: caps.fs_write?.length ?? 0 }),
                            caps.spawn ? tn(caps.spawn.max_children, 'can create {n} child', 'can create {n} children') : t('no children'),
                          ].join(', ')}
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
