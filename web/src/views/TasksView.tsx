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
import type { Cluster } from '@/lib/store'
import type { Task, TasksData } from '@/lib/types'

const DAYS = ['lundi', 'mardi', 'mercredi', 'jeudi', 'vendredi', 'samedi', 'dimanche']
const two = (n: number) => String(n).padStart(2, '0')
const clock = (minute: number) => `${two(Math.floor(minute / 60))}:${two(minute % 60)}`
/** Le décalage de ce navigateur par rapport à UTC, en minutes (Paris l'hiver : 60). */
const offset = () => -new Date().getTimezoneOffset()

function rhythm(task: Task) {
  if (task.kind === 'hours') return task.hours === 1 ? 'toutes les heures' : `toutes les ${task.hours} heures`
  if (task.kind === 'daily') return `chaque jour à ${clock(task.minute ?? 0)}`
  return `chaque ${DAYS[task.weekday ?? 0]} à ${clock(task.minute ?? 0)}`
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
  const asDraft = (t: Task): Draft => ({ id: t.id, name: t.name, text: t.text, kind: t.kind, time: clock(t.minute ?? 420), weekday: t.weekday ?? 0, hours: t.hours ?? 6, enabled: t.enabled })

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!draft) return
    const path = draft.id ? `/api/tasks/${encodeURIComponent(draft.id)}` : '/api/tasks'
    if (await run(() => api(cluster.token, path, body(draft)), draft.id ? 'Tâche modifiée.' : 'Tâche créée.')) setDraft(null)
  }

  const tasks = data?.tasks ?? []
  const full = data !== null && tasks.length >= data.max
  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        eyebrow="Maîtrise · tâches"
        title="Ce que le maître fait de lui-même, à heure fixe"
        lead="Une tâche planifiée est une consigne écrite d'avance : le maître la reçoit à l'heure dite, comme si vous veniez de la lui écrire. Elle ne lui donne aucun droit de plus. Les agents ne peuvent ni en créer, ni en changer."
      >
        {!draft && (
          <Button onClick={() => setDraft(blank)} disabled={full} title={full ? `Pas plus de ${data?.max} tâches` : undefined}>
            <Plus /> Nouvelle tâche
          </Button>
        )}
      </PageHeader>

      {draft && (
        <form onSubmit={submit} className="flex flex-col gap-4 rounded-lg border bg-card p-5">
          <p className="font-semibold">{draft.id ? 'Modifier la tâche' : 'Nouvelle tâche'}</p>
          <div className="flex flex-col gap-2">
            <Label htmlFor="task-name">Nom</Label>
            <Input id="task-name" required maxLength={60} value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="Catalogue des modèles" />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="task-text">Consigne remise au maître</Label>
            <Textarea id="task-text" required rows={5} value={draft.text} onChange={(e) => setDraft({ ...draft, text: e.target.value })} placeholder="Ce que le maître doit faire, et comment savoir que c'est fait." />
          </div>
          <div className="flex flex-wrap items-end gap-4">
            <div className="flex flex-col gap-2">
              <Label htmlFor="task-kind">Rythme</Label>
              <select
                id="task-kind"
                value={draft.kind}
                onChange={(e) => setDraft({ ...draft, kind: e.target.value as Task['kind'] })}
                className="h-9 rounded-md border bg-transparent px-3 text-sm"
              >
                <option value="daily">Chaque jour</option>
                <option value="weekly">Chaque semaine</option>
                <option value="hours">Toutes les N heures</option>
              </select>
            </div>
            {draft.kind === 'weekly' && (
              <div className="flex flex-col gap-2">
                <Label htmlFor="task-day">Jour</Label>
                <select id="task-day" value={draft.weekday} onChange={(e) => setDraft({ ...draft, weekday: Number(e.target.value) })} className="h-9 rounded-md border bg-transparent px-3 text-sm">
                  {DAYS.map((name, i) => (
                    <option key={name} value={i}>{name}</option>
                  ))}
                </select>
              </div>
            )}
            {draft.kind !== 'hours' && (
              <div className="flex flex-col gap-2">
                <Label htmlFor="task-time">Heure (la vôtre)</Label>
                <Input id="task-time" type="time" required value={draft.time} onChange={(e) => setDraft({ ...draft, time: e.target.value })} className="w-32" />
              </div>
            )}
            {draft.kind === 'hours' && (
              <div className="flex flex-col gap-2">
                <Label htmlFor="task-hours">Heures entre deux fois</Label>
                <Input id="task-hours" type="number" min={1} max={744} required value={draft.hours} onChange={(e) => setDraft({ ...draft, hours: Number(e.target.value) })} className="w-32" />
              </div>
            )}
            <Label className="h-9 font-normal">
              <Checkbox checked={draft.enabled} onCheckedChange={(checked) => setDraft({ ...draft, enabled: checked === true })} />
              Activée
            </Label>
          </div>
          <p className="text-[13px] text-muted-foreground">
            L'heure est celle de ce navigateur ; elle ne suit pas le changement d'heure d'été. Une échéance passée pendant que le maître était éteint part une fois à son retour.
          </p>
          <div className="flex gap-2">
            <Button type="submit" disabled={busy}>{draft.id ? 'Enregistrer' : 'Créer'}</Button>
            <Button type="button" variant="outline" onClick={() => setDraft(null)}>Annuler</Button>
          </div>
        </form>
      )}

      {error && <p className="text-destructive" role="alert">{error}</p>}
      {done && <p className="text-muted-foreground" role="status">{done}</p>}

      {data !== null && tasks.length === 0 && !draft && (
        <Empty title="Aucune tâche planifiée">Créez-en une pour que le maître lance seul un travail régulier : une veille, une mise à jour, un contrôle.</Empty>
      )}
      {tasks.length > 0 && (
        <div className="rounded-lg border bg-card">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="eyebrow px-4">Tâche</TableHead>
                <TableHead className="eyebrow">Rythme</TableHead>
                <TableHead className="eyebrow">Prochaine fois</TableHead>
                <TableHead className="eyebrow">Dernière fois</TableHead>
                <TableHead className="eyebrow px-4 text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {tasks.map((task) => (
                <TableRow key={task.id} className="hover:bg-transparent [&>td]:align-top">
                  <TableCell className="max-w-md px-4 whitespace-normal">
                    <span className="flex items-center gap-2 font-medium">
                      <span className={`size-2 rounded-full ${task.enabled ? 'bg-st-working' : 'ring-1 ring-st-off ring-inset'}`} aria-hidden />
                      {task.name}
                      {!task.enabled && <span className="text-[13px] font-normal text-muted-foreground">en pause</span>}
                    </span>
                    <p className="mt-1 line-clamp-2 text-[13px] text-muted-foreground [overflow-wrap:anywhere]" title={task.text.slice(0, 600)}>{task.text}</p>
                  </TableCell>
                  <TableCell className="whitespace-nowrap">{rhythm(task)}</TableCell>
                  <TableCell className="num whitespace-nowrap">{task.enabled ? moment(task.next_run_ms) : '—'}</TableCell>
                  <TableCell className="num whitespace-nowrap">{task.last_run_ms ? moment(task.last_run_ms) : 'jamais'}</TableCell>
                  <TableCell className="px-4">
                    <span className="flex justify-end gap-1.5">
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy}
                        onClick={() => run(() => api(cluster.token, `/api/tasks/${encodeURIComponent(task.id)}/run`, {}), `« ${task.name} » remise au maître : suivez-la dans le dialogue.`)}
                      >
                        <Play /> Lancer
                      </Button>
                      <Button variant="outline" size="icon-sm" disabled={busy} aria-label={`Modifier ${task.name}`} title="Modifier" onClick={() => setDraft(asDraft(task))}>
                        <Pencil />
                      </Button>
                      <Button
                        variant="outline"
                        size="icon-sm"
                        disabled={busy}
                        aria-label={`Supprimer ${task.name}`}
                        title="Supprimer"
                        onClick={() => {
                          if (confirm(`Supprimer la tâche « ${task.name} » ?`)) void run(() => api(cluster.token, `/api/tasks/${encodeURIComponent(task.id)}/delete`, {}), 'Tâche supprimée.')
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
