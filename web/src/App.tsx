import { useEffect, useState, type FormEvent } from 'react'
import { BookUser, Boxes, Building2, CalendarClock, History, KeyRound, Languages, LogOut, Menu, MessageSquare, Moon, Network, ScrollText, Sun, Terminal } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Guard, Rich } from '@/components/bits'
import { flat } from '@/lib/format'
import { LANGS, setLang, t, tx, useLang, type Lang } from '@/lib/i18n'
import { useCluster, type Cluster } from '@/lib/store'
import { cn } from '@/lib/utils'
import { ChatView } from '@/views/ChatView'
import { CommandsView } from '@/views/CommandsView'
import { JournalView } from '@/views/JournalView'
import { ModelsView } from '@/views/ModelsView'
import { ProvidersView } from '@/views/ProvidersView'
import { ReleasesView } from '@/views/ReleasesView'
import { RolesView } from '@/views/RolesView'
import { SecretsView } from '@/views/SecretsView'
import { TasksView } from '@/views/TasksView'
import { TreeView } from '@/views/TreeView'

const VIEWS = [
  { id: 'grappe', label: tx('Cluster'), icon: Network, group: 'follow' },
  { id: 'dialogue', label: tx('Conversation'), icon: MessageSquare, group: 'follow' },
  { id: 'journal', label: tx('Journal'), icon: ScrollText, group: 'follow' },
  { id: 'commandes', label: tx('Commands'), icon: Terminal, group: 'control' },
  { id: 'fournisseurs', label: tx('Providers'), icon: Building2, group: 'control' },
  { id: 'modeles', label: tx('Models'), icon: Boxes, group: 'control' },
  { id: 'roles', label: tx('Roles'), icon: BookUser, group: 'control' },
  { id: 'taches', label: tx('Tasks'), icon: CalendarClock, group: 'control' },
  { id: 'secrets', label: tx('Secrets'), icon: KeyRound, group: 'control' },
  { id: 'nouveautes', label: tx("What's new"), icon: History, group: 'version' },
] as const
type ViewId = (typeof VIEWS)[number]['id']

const GROUPS = [
  { id: 'follow', label: tx('Follow') },
  { id: 'control', label: tx('Control') },
  { id: 'version', label: tx('Version') },
] as const

const viewFromHash = (): ViewId => VIEWS.find((v) => v.id === location.hash.slice(1).split('?')[0])?.id ?? 'grappe'

function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 512 512" className={className} aria-hidden>
      <rect width="512" height="512" rx="112" className="fill-foreground" />
      <path d="M256 150v106M256 256H148v94M256 256h108v94" fill="none" strokeWidth="26" strokeLinecap="square" className="stroke-background" />
      <rect x="212" y="106" width="88" height="88" rx="20" className="fill-signal" />
      <rect x="112" y="330" width="72" height="72" rx="18" className="fill-background" />
      <rect x="328" y="330" width="72" height="72" rx="18" className="fill-background" />
    </svg>
  )
}

function useTheme() {
  const [dark, setDark] = useState(() => {
    const saved = localStorage.getItem('talixio-theme')
    return saved ? saved === 'dark' : window.matchMedia('(prefers-color-scheme: dark)').matches
  })
  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])
  return {
    dark,
    toggle: () =>
      setDark((d) => {
        localStorage.setItem('talixio-theme', d ? 'light' : 'dark')
        return !d
      }),
  }
}

export default function App() {
  const theme = useTheme()
  // every text is read again when the language changes
  const lang = useLang()
  const { cluster, waiting, login, loginError, logout } = useCluster()
  if (cluster) return <Shell key={lang} cluster={cluster} logout={logout} theme={theme} />
  if (waiting) return <div className="grid h-full place-items-center"><Logo className="size-10 animate-pulse" /></div>
  return <Login key={lang} login={login} error={loginError} />
}

function Login({ login, error }: { login: (token: string, remember: boolean) => Promise<void>; error: string | null }) {
  const [token, setToken] = useState('')
  const [remember, setRemember] = useState(false)
  const [busy, setBusy] = useState(false)
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setBusy(true)
    await login(token.trim(), remember)
    setBusy(false)
  }
  return (
    <main className="grid min-h-full place-items-center p-6">
      <form onSubmit={submit} className="flex w-full max-w-sm flex-col gap-5">
        <div className="flex items-center gap-3">
          <Logo className="size-9" />
          <div>
            <p className="text-lg leading-tight font-semibold tracking-tight">Talixio Agents</p>
            <p className="eyebrow">{t('control room')}</p>
          </div>
        </div>
        <div className="flex flex-col gap-4 rounded-lg border bg-card p-5">
          <div className="flex flex-col gap-2">
            <Label htmlFor="token">{t('API token')}</Label>
            <Input id="token" type="password" autoComplete="current-password" required autoFocus value={token} onChange={(e) => setToken(e.target.value)} className="font-mono" />
            <p className="text-[13px] text-muted-foreground">
              <Rich text={t('It is on the master machine, in `.talixio/api-token`. Started with `TALIXIO_API_TOKEN` (Docker): it is the value of that variable, and the file does not exist.')} />
            </p>
          </div>
          <Label className="font-normal">
            <Checkbox checked={remember} onCheckedChange={(checked) => setRemember(checked === true)} />
            {t('Stay signed in on this device')}
          </Label>
          {error && <p className="text-destructive" role="alert">{error}</p>}
          <Button type="submit" size="lg" disabled={busy || !token.trim()}>{t('Enter')}</Button>
        </div>
        <LangPicker className="self-center" />
      </form>
    </main>
  )
}

/** The language of the interface: kept on this device, the browser's language the first time. */
function LangPicker({ className }: { className?: string }) {
  const lang = useLang()
  return (
    <label className={cn('inline-flex items-center gap-1.5 text-xs text-muted-foreground', className)}>
      <Languages className="size-3.5" aria-hidden />
      <span className="sr-only">{t('Language')}</span>
      <select
        value={lang}
        onChange={(e) => setLang(e.target.value as Lang)}
        className="rounded-md bg-transparent py-1 outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        {LANGS.map((l) => (
          <option key={l.id} value={l.id} lang={l.id}>{l.label}</option>
        ))}
      </select>
    </label>
  )
}

function Shell({ cluster, logout, theme }: { cluster: Cluster; logout: () => void; theme: ReturnType<typeof useTheme> }) {
  const [view, setView] = useState<ViewId>(viewFromHash)
  const [menu, setMenu] = useState(false)
  useEffect(() => {
    const onHash = () => setView(viewFromHash())
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])
  const go = (id: ViewId) => {
    location.hash = id
    setView(id)
    setMenu(false)
    window.scrollTo(0, 0)
  }
  const working = flat(cluster.tree).filter((a) => a.status === 'working').length
  const current = VIEWS.find((v) => v.id === view)!

  const nav = (
    <nav className="flex flex-1 flex-col gap-5" aria-label="Navigation">
      {GROUPS.map((group) => (
        <div key={group.id} className="flex flex-col gap-0.5">
          <p className="eyebrow px-3 pb-1.5">{t(group.label)}</p>
          {VIEWS.filter((v) => v.group === group.id).map((item) => (
            <button
              key={item.id}
              type="button"
              onClick={() => go(item.id)}
              aria-current={view === item.id ? 'page' : undefined}
              className={cn(
                'flex h-9 items-center gap-2.5 rounded-md px-3 text-left text-sidebar-foreground/75 outline-none transition-colors hover:bg-sidebar-accent hover:text-sidebar-foreground focus-visible:ring-3 focus-visible:ring-ring/50',
                view === item.id && 'bg-sidebar-accent font-medium text-sidebar-foreground shadow-[inset_3px_0_0_var(--signal)]',
              )}
            >
              <item.icon className="size-4" />
              {t(item.label)}
              {item.id === 'grappe' && working > 0 && <span className="num ml-auto inline-flex items-center gap-1.5 text-xs"><span className="live-dot size-1.5 rounded-full bg-st-working" />{working}</span>}
              {item.id === 'nouveautes' && cluster.releases && <span className="num ml-auto text-xs text-muted-foreground">v{cluster.releases.version}</span>}
            </button>
          ))}
        </div>
      ))}
      <div className="mt-auto flex items-center gap-1 border-t pt-3">
        <Button variant="ghost" size="sm" onClick={logout} className="text-muted-foreground"><LogOut /> {t('Sign out')}</Button>
        <LangPicker className="ml-auto" />
        <Button variant="ghost" size="icon-sm" onClick={theme.toggle} className="text-muted-foreground" aria-label={theme.dark ? t('Switch to light') : t('Switch to dark')}>
          {theme.dark ? <Sun /> : <Moon />}
        </Button>
      </div>
    </nav>
  )
  const brand = (
    <div className="flex items-center gap-2.5">
      <Logo className="size-7" />
      <span className="font-semibold tracking-tight">Talixio Agents</span>
    </div>
  )

  return (
    <div className="flex min-h-full flex-col lg:flex-row">
      <aside className="sticky top-0 hidden h-dvh w-60 shrink-0 flex-col gap-6 border-r bg-sidebar px-3 py-5 lg:flex">
        <div className="px-3">{brand}</div>
        {nav}
      </aside>
      <header className="sticky top-0 z-10 flex items-center gap-3 border-b bg-background/90 px-4 py-2.5 backdrop-blur lg:hidden">
        <Logo className="size-7" />
        <span className="font-semibold">{t(current.label)}</span>
        <Button variant="outline" size="icon" className="ml-auto" onClick={() => setMenu(true)} aria-label={t('Open the menu')}><Menu /></Button>
      </header>
      <Sheet open={menu} onOpenChange={setMenu}>
        <SheetContent side="left" className="w-72 gap-6 bg-sidebar px-3 py-5">
          <SheetHeader className="px-3 py-0">
            <SheetTitle render={<div />}>{brand}</SheetTitle>
            <SheetDescription className="sr-only">Navigation</SheetDescription>
          </SheetHeader>
          {nav}
        </SheetContent>
      </Sheet>
      <main className="mx-auto w-full max-w-[1400px] min-w-0 flex-1 px-4 py-6 lg:px-10 lg:py-9">
        {cluster.offline && (
          <p role="status" className="mb-5 rounded-md border border-st-wait/50 bg-st-wait/10 px-3 py-2">{t('Master unreachable. Trying again…')}</p>
        )}
        <Guard key={view}>
        {view === 'grappe' && <TreeView cluster={cluster} />}
        {view === 'dialogue' && <ChatView cluster={cluster} />}
        {view === 'journal' && <JournalView cluster={cluster} />}
        {view === 'commandes' && <CommandsView cluster={cluster} />}
        {view === 'fournisseurs' && <ProvidersView cluster={cluster} />}
        {view === 'modeles' && <ModelsView cluster={cluster} />}
        {view === 'roles' && <RolesView cluster={cluster} />}
        {view === 'taches' && <TasksView cluster={cluster} />}
        {view === 'secrets' && <SecretsView cluster={cluster} />}
        {view === 'nouveautes' && <ReleasesView cluster={cluster} />}
        </Guard>
      </main>
    </div>
  )
}
