import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { Pencil, Play, Plus, Trash2 } from 'lucide-react'
import { Empty, PageHeader } from '@/components/bits'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Textarea } from '@/components/ui/textarea'
import { api } from '@/lib/api'
import { moment } from '@/lib/format'
import { t, tx } from '@/lib/i18n'
import type { Cluster } from '@/lib/store'
import type { Task, TasksData } from '@/lib/types'

const DAYS = [tx('Monday'), tx('Tuesday'), tx('Wednesday'), tx('Thursday'), tx('Friday'), tx('Saturday'), tx('Sunday')]
const two = (n: number) => String(n).padStart(2, '0')
const clock = (minute: number) => `${two(Math.floor(minute / 60))}:${two(minute % 60)}`
/** This browser's offset from UTC, in minutes (Paris in winter: 60). */
const offset = () => -new Date().getTimezoneOffset()

function rhythm(task: Task) {
  if (task.kind === 'hours') return task.hours === 1 ? t('every hour') : t('every {hours} hours', { hours: task.hours ?? 0 })
  if (task.kind === 'daily') return t('every day at {time}', { time: clock(task.minute ?? 0) })
  return t('every {day} at {time}', { day: t(DAYS[task.weekday ?? 0]), time: clock(task.minute ?? 0) })
}

interface Draft {
  id: string | null
  name: string
  text: string
  kind: Task['kind']
  time: string
  weekday: number
  hours: number
  enabled: boolean
}

const blank: Draft = { id: null, name: '', text: '', kind: 'weekly', time: '07:00', weekday: 0, hours: 6, enabled: true }

export function TasksView({ cluster }: { cluster: Cluster }) {
  const [data, setData] = useState<TasksData | null>(null)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [done, setDone] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      setData(await api<TasksData>(cluster.token, '/api/tasks'))
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

  const body = (d: Draft) => ({ name: d.name, text: d.text, kind: d.kind, time: d.time, weekday: d.weekday, hours: d.hours, enabled: d.enabled, tz_offset_min: offset() })
  const asDraft = (task: Task): Draft => ({ id: task.id, name: task.name, text: task.text, kind: task.kind, time: clock(task.minute ?? 420), weekday: task.weekday ?? 0, hours: task.hours ?? 6, enabled: task.enabled })

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!draft) return
    const path = draft.id ? `/api/tasks/${encodeURIComponent(draft.id)}` : '/api/tasks'
    if (await run(() => api(cluster.token, path, body(draft)), draft.id ? t('Task updated.') : t('Task created.'))) setDraft(null)
  }

  const tasks = data?.tasks ?? []
  const full = data !== null && tasks.length >= data.max
  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow={t('Control · tasks')}
        title={t('What the master does on its own, at set times')}
        lead={t("A scheduled task is an instruction written in advance: the master receives it at the set time, as if you had just written it. It gives the master no extra permission. Agents can neither create nor change one.")}
      >
        {!draft && (
          <Button onClick={() => setDraft(blank)} disabled={full} title={full ? t('No more than {max} tasks', { max: data?.max ?? 0 }) : undefined}>
            <Plus /> {t('New task')}
          </Button>
        )}
      </PageHeader>

      {draft && (
        <form onSubmit={submit} className="flex flex-col gap-4 rounded-lg border bg-card p-5">
          <p className="font-semibold">{draft.id ? t('Edit the task') : t('New task')}</p>
          <div className="flex flex-col gap-2">
            <Label htmlFor="task-name">{t('Name')}</Label>
            <Input id="task-name" required maxLength={60} value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder={t('Model catalogue')} />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="task-text">{t('Instruction handed to the master')}</Label>
            <Textarea id="task-text" required rows={5} value={draft.text} onChange={(e) => setDraft({ ...draft, text: e.target.value })} placeholder={t("What the master must do, and how to know it is done.")} />
          </div>
          <div className="flex flex-wrap items-end gap-4">
            <div className="flex flex-col gap-2">
              <Label htmlFor="task-kind">{t('Frequency')}</Label>
              <select
                id="task-kind"
                value={draft.kind}
                onChange={(e) => setDraft({ ...draft, kind: e.target.value as Task['kind'] })}
                className="h-9 rounded-md border bg-transparent px-3 text-sm"
              >
                <option value="daily">{t('Every day')}</option>
                <option value="weekly">{t('Every week')}</option>
                <option value="hours">{t('Every N hours')}</option>
              </select>
            </div>
            {draft.kind === 'weekly' && (
              <div className="flex flex-col gap-2">
                <Label htmlFor="task-day">{t('Day')}</Label>
                <select id="task-day" value={draft.weekday} onChange={(e) => setDraft({ ...draft, weekday: Number(e.target.value) })} className="h-9 rounded-md border bg-transparent px-3 text-sm">
                  {DAYS.map((name, i) => (
                    <option key={name} value={i}>{t(name)}</option>
                  ))}
                </select>
              </div>
            )}
            {draft.kind !== 'hours' && (
              <div className="flex flex-col gap-2">
                <Label htmlFor="task-time">{t('Time (yours)')}</Label>
                <Input id="task-time" type="time" required value={draft.time} onChange={(e) => setDraft({ ...draft, time: e.target.value })} className="w-32" />
              </div>
            )}
            {draft.kind === 'hours' && (
              <div className="flex flex-col gap-2">
                <Label htmlFor="task-hours">{t('Hours between two runs')}</Label>
                <Input id="task-hours" type="number" min={1} max={744} required value={draft.hours} onChange={(e) => setDraft({ ...draft, hours: Number(e.target.value) })} className="w-32" />
              </div>
            )}
            <Label className="h-9 font-normal">
              <Checkbox checked={draft.enabled} onCheckedChange={(checked) => setDraft({ ...draft, enabled: checked === true })} />
              {t('Task enabled')}
            </Label>
          </div>
          <p className="text-[13px] text-muted-foreground">
            {t("The time is this browser's; it does not follow daylight saving changes. A run missed while the master was off starts once when it is back.")}
          </p>
          <div className="flex gap-2">
            <Button type="submit" disabled={busy}>{draft.id ? t('Save') : t('Create')}</Button>
            <Button type="button" variant="outline" onClick={() => setDraft(null)}>{t('Cancel')}</Button>
          </div>
        </form>
      )}

      {error && <p className="text-destructive" role="alert">{error}</p>}
      {done && <p className="text-muted-foreground" role="status">{done}</p>}

      {data !== null && tasks.length === 0 && !draft && (
        <Empty title={t('No scheduled task')}>{t('Create one so that the master starts a regular job on its own: a watch, an update, a check.')}</Empty>
      )}
      {tasks.length > 0 && (
        <div className="rounded-lg border bg-card">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="eyebrow px-4">{t('Task')}</TableHead>
                <TableHead className="eyebrow">{t('Frequency')}</TableHead>
                <TableHead className="eyebrow">{t('Next run')}</TableHead>
                <TableHead className="eyebrow">{t('Last run')}</TableHead>
                <TableHead className="eyebrow px-4 text-right">{t('Actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {tasks.map((task) => (
                <TableRow key={task.id} className="hover:bg-transparent [&>td]:align-top">
                  <TableCell className="max-w-md px-4 whitespace-normal">
                    <span className="flex items-center gap-2 font-medium">
                      <span className={`size-2 rounded-full ${task.enabled ? 'bg-st-working' : 'ring-1 ring-st-off ring-inset'}`} aria-hidden />
                      {task.name}
                      {!task.enabled && <span className="text-[13px] font-normal text-muted-foreground">{t('paused')}</span>}
                    </span>
                    <p className="mt-1 line-clamp-2 text-[13px] text-muted-foreground [overflow-wrap:anywhere]" title={task.text.slice(0, 600)}>{task.text}</p>
                  </TableCell>
                  <TableCell className="whitespace-nowrap">{rhythm(task)}</TableCell>
                  <TableCell className="num whitespace-nowrap">{task.enabled ? moment(task.next_run_ms) : '—'}</TableCell>
                  <TableCell className="num whitespace-nowrap">{task.last_run_ms ? moment(task.last_run_ms) : t('never')}</TableCell>
                  <TableCell className="px-4">
                    <span className="flex justify-end gap-1.5">
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy}
                        onClick={() => run(() => api(cluster.token, `/api/tasks/${encodeURIComponent(task.id)}/run`, {}), t('“{name}” handed to the master: follow it in the conversation.', { name: task.name }))}
                      >
                        <Play /> {t('Run')}
                      </Button>
                      <Button variant="outline" size="icon-sm" disabled={busy} aria-label={t('Edit {name}', { name: task.name })} title={t('Edit')} onClick={() => setDraft(asDraft(task))}>
                        <Pencil />
                      </Button>
                      <Button
                        variant="outline"
                        size="icon-sm"
                        disabled={busy}
                        aria-label={t('Delete {name}', { name: task.name })}
                        title={t('Delete')}
                        onClick={() => {
                          if (confirm(t('Delete the task “{name}”?', { name: task.name }))) void run(() => api(cluster.token, `/api/tasks/${encodeURIComponent(task.id)}/delete`, {}), t('Task deleted.'))
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
