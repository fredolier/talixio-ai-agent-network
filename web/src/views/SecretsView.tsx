import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { Pencil, Plus, Power, PowerOff, Trash2 } from 'lucide-react'
import { Empty, PageHeader } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { api } from '@/lib/api'
import { moment } from '@/lib/format'
import { t } from '@/lib/i18n'
import type { Cluster } from '@/lib/store'
import type { Secret, SecretsData } from '@/lib/types'

interface Draft {
  /** Set when an existing secret is edited: its command and variable no longer change. */
  editing: boolean
  name: string
  value: string
  command: string
  variable: string
  roles: string[]
  master: boolean
  remote: boolean
  env: Record<string, string>
}

export function SecretsView({ cluster }: { cluster: Cluster }) {
  const [data, setData] = useState<SecretsData | null>(null)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [done, setDone] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      setData(await api<SecretsData>(cluster.token, '/api/secrets'))
    } catch (e) {
      setError((e as Error).message)
    }
  }, [cluster.token])
  useEffect(() => {
    void load()
    const timer = setInterval(load, 15000)
    return () => clearInterval(timer)
  }, [load])

  const run = async (action: () => Promise<unknown>, said: string) => {
    setBusy(true)
    setError(null)
    setDone(null)
    try {
      await action()
      setDone(said)
      await load()
      return true
    } catch (e) {
      setError((e as Error).message)
      return false
    } finally {
      setBusy(false)
    }
  }

  const commands = data?.commands ?? []
  const secrets = data?.secrets ?? []
  const blank = (): Draft => {
    const first = commands[0]
    return { editing: false, name: '', value: '', command: first?.name ?? '', variable: first?.secrets[0] ?? '', roles: [], master: false, remote: false, env: {} }
  }
  const asDraft = (s: Secret): Draft => ({ editing: true, name: s.name, value: '', command: s.command, variable: s.variable, roles: s.roles, master: s.master, remote: s.remote, env: { ...s.env } })
  const path = (name: string) => `/api/secrets/${encodeURIComponent(name)}`

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!draft) return
    const body = { name: draft.name, value: draft.value, command: draft.command, variable: draft.variable, roles: draft.roles, master: draft.master, remote: draft.remote, env: draft.env }
    const ok = await run(() => api(cluster.token, draft.editing ? path(draft.name) : '/api/secrets', body), draft.editing ? t('Secret updated.') : t('Secret added. Its value will not be shown again.'))
    if (ok) setDraft(null)
  }

  const command = commands.find((c) => c.name === draft?.command)
  const who = (s: Secret) => [...s.roles, ...(s.master ? [t('the master')] : [])].join(', ')
  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow={t('Control · secrets')}
        title={t('Tokens and passwords for commands')}
        lead={t('A secret goes to one command of an installed pack, for the time of one run: never into the instructions, the tools or the journal of an agent, and it is removed from what the command prints. You decide here which command receives it and for which roles. The master keeps the value in its state directory, closed to other accounts, and never shows it again; disabling it takes effect at the next run.')}
      >
        {!draft && commands.length > 0 && (
          <Button onClick={() => setDraft(blank())}>
            <Plus /> {t('New secret')}
          </Button>
        )}
      </PageHeader>

      {draft && (
        <form onSubmit={submit} className="flex flex-col gap-4 rounded-lg border bg-card p-5" autoComplete="off">
          <p className="font-semibold">{draft.editing ? t('Edit the secret “{name}”', { name: draft.name }) : t('New secret')}</p>
          {!draft.editing && (
            <div className="flex flex-col gap-2">
              <Label htmlFor="secret-name">{t('Name')}</Label>
              <Input id="secret-name" required maxLength={40} pattern="[a-z0-9][a-z0-9-]*" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="gitlab-read" className="max-w-sm" />
              <p className="text-[13px] text-muted-foreground">{t('Lowercase letters, digits and dashes. The name appears in the journal; the value never does.')}</p>
            </div>
          )}
          <div className="flex flex-col gap-2">
            <Label htmlFor="secret-value">{draft.editing ? t('New value (leave empty to keep the current one)') : t('Value')}</Label>
            <Input id="secret-value" type="password" autoComplete="new-password" required={!draft.editing} minLength={8} maxLength={4096} value={draft.value} onChange={(e) => setDraft({ ...draft, value: e.target.value })} className="max-w-xl font-mono" />
          </div>
          <div className="flex flex-wrap items-end gap-4">
            <div className="flex flex-col gap-2">
              <Label htmlFor="secret-command">{t('Command that receives it')}</Label>
              <select
                id="secret-command"
                disabled={draft.editing}
                value={draft.command}
                onChange={(e) => {
                  const next = commands.find((c) => c.name === e.target.value)
                  setDraft({ ...draft, command: e.target.value, variable: next?.secrets[0] ?? '', roles: [], env: {} })
                }}
                className="h-9 rounded-md border bg-transparent px-3 text-sm"
              >
                {!command && <option value={draft.command}>{draft.command}</option>}
                {commands.map((c) => (
                  <option key={c.name} value={c.name}>{c.name}</option>
                ))}
              </select>
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="secret-variable">{t('In the variable')}</Label>
              <select id="secret-variable" disabled={draft.editing} value={draft.variable} onChange={(e) => setDraft({ ...draft, variable: e.target.value })} className="h-9 rounded-md border bg-transparent px-3 font-mono text-sm">
                {!command && <option value={draft.variable}>{draft.variable}</option>}
                {command?.secrets.map((name) => (
                  <option key={name} value={name}>{name}</option>
                ))}
              </select>
            </div>
          </div>
          {!command && <p className="text-destructive">{t('The pack of this command is no longer installed: the secret can only be disabled or deleted.')}</p>}
          {command && command.env.length > 0 && (
            <div className="flex flex-col gap-3">
              <p className="text-[13px] text-muted-foreground">{t('Settings that travel with the secret. They are not secret: they tell the command where the secret is valid.')}</p>
              {command.env.map((name) => (
                <div key={name} className="flex flex-col gap-2">
                  <Label htmlFor={`secret-env-${name}`} className="font-mono">{name}</Label>
                  <Input id={`secret-env-${name}`} maxLength={300} value={draft.env[name] ?? ''} onChange={(e) => setDraft({ ...draft, env: { ...draft.env, [name]: e.target.value } })} className="max-w-xl" />
                </div>
              ))}
            </div>
          )}
          <fieldset className="flex flex-col gap-2">
            <legend className="mb-2 text-sm font-medium">{t('Who may use it')}</legend>
            {command?.roles.map((role) => (
              <Label key={role} className="font-normal">
                <Checkbox checked={draft.roles.includes(role)} onCheckedChange={(checked) => setDraft({ ...draft, roles: checked === true ? [...draft.roles, role] : draft.roles.filter((r) => r !== role) })} />
                <span className="font-mono">{role}</span>
              </Label>
            ))}
            <Label className="font-normal">
              <Checkbox checked={draft.master} onCheckedChange={(checked) => setDraft({ ...draft, master: checked === true })} />
              {t('The master itself')}
            </Label>
            <p className="text-[13px] text-muted-foreground">{t('Only agents created with one of these roles receive it, when they run this command. A role is given by its parent: tie a secret to a role only if you trust every agent that may create it.')}</p>
          </fieldset>
          <div className="flex flex-col gap-2">
            <Label className="font-normal">
              <Checkbox checked={draft.remote} onCheckedChange={(checked) => setDraft({ ...draft, remote: checked === true })} />
              {t('Also to agents on other machines (ssh targets)')}
            </Label>
            <p className="text-[13px] text-muted-foreground">{t('Off by default: over there the secret exists in the memory of an account the master does not control.')}</p>
          </div>
          <div className="flex gap-2">
            <Button type="submit" disabled={busy}>{draft.editing ? t('Save') : t('Add')}</Button>
            <Button type="button" variant="outline" onClick={() => setDraft(null)}>{t('Cancel')}</Button>
          </div>
        </form>
      )}

      {error && <p className="text-destructive" role="alert">{error}</p>}
      {done && <p className="text-muted-foreground" role="status">{done}</p>}

      {data !== null && commands.length === 0 && secrets.length === 0 && (
        <Empty title={t('No command accepts a secret')}>{t('A secret is tied to a command of an installed pack that declares it, such as fetch-repo of the audit pack. Install such a pack, restart the master, and come back here.')}</Empty>
      )}
      {data !== null && commands.length > 0 && secrets.length === 0 && !draft && (
        <Empty title={t('No secret')}>{t('Add one so that a command can reach what needs a token: a private repository, an API.')}</Empty>
      )}
      {secrets.length > 0 && (
        <div className="rounded-lg border bg-card">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="eyebrow px-4">{t('Secret')}</TableHead>
                <TableHead className="eyebrow">{t('Goes to')}</TableHead>
                <TableHead className="eyebrow">{t('For')}</TableHead>
                <TableHead className="eyebrow">{t('Last given')}</TableHead>
                <TableHead className="eyebrow px-4 text-right">{t('Actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {secrets.map((s) => (
                <TableRow key={s.name} className="hover:bg-transparent [&>td]:align-top">
                  <TableCell className="px-4">
                    <span className="flex items-center gap-2 font-medium">
                      <span className={`size-2 rounded-full ${s.enabled ? 'bg-st-working' : 'ring-1 ring-st-off ring-inset'}`} aria-hidden />
                      {s.name}
                      {!s.enabled && <span className="text-[13px] font-normal text-muted-foreground">{t('disabled')}</span>}
                    </span>
                    {s.stale && <p className="mt-1 text-[13px] text-destructive whitespace-normal">{t('The command changed since this secret was saved: it is no longer given. Open it and save it to confirm.')}</p>}
                  </TableCell>
                  <TableCell className="whitespace-normal">
                    <span className="font-mono text-[13px]">{s.variable}</span> {t('of')} <span className="font-mono text-[13px]">{s.command}</span>
                    {Object.entries(s.env).map(([name, value]) => (
                      <p key={name} className="mt-1 text-[13px] text-muted-foreground [overflow-wrap:anywhere]">
                        <span className="font-mono">{name}</span> = {value}
                      </p>
                    ))}
                  </TableCell>
                  <TableCell className="whitespace-normal">
                    {who(s)}
                    {s.remote && <p className="mt-1 text-[13px] text-muted-foreground">{t('other machines included')}</p>}
                  </TableCell>
                  <TableCell className="num whitespace-nowrap">
                    {s.used_ms ? moment(s.used_ms) : t('never')}
                    {s.used_ms > 0 && <p className="mt-1 font-mono text-[13px] text-muted-foreground">{s.used_by}</p>}
                  </TableCell>
                  <TableCell className="px-4">
                    <span className="flex justify-end gap-1.5">
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy}
                        onClick={() => run(() => api(cluster.token, path(s.name), { enabled: !s.enabled }), s.enabled ? t('“{name}” disabled: no command receives it any more.', { name: s.name }) : t('“{name}” enabled.', { name: s.name }))}
                      >
                        {s.enabled ? <PowerOff /> : <Power />} {s.enabled ? t('Disable') : t('Enable')}
                      </Button>
                      <Button variant="outline" size="icon-sm" disabled={busy} aria-label={t('Edit {name}', { name: s.name })} title={t('Edit')} onClick={() => setDraft(asDraft(s))}>
                        <Pencil />
                      </Button>
                      <Button
                        variant="outline"
                        size="icon-sm"
                        disabled={busy}
                        aria-label={t('Delete {name}', { name: s.name })}
                        title={t('Delete')}
                        onClick={() => {
                          if (confirm(t('Delete the secret “{name}”? Its file is removed from the master.', { name: s.name }))) void run(() => api(cluster.token, `${path(s.name)}/delete`, {}), t('Secret deleted.'))
                        }}
                      >
                        <Trash2 />
                      </Button>
                    </span>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  )
}
