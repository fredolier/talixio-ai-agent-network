// Accès à l'API du maître. Le jeton reste dans le navigateur : session, ou appareil si demandé.

const KEY = 'talixio-token'

export class Unauthorized extends Error {}

export const savedToken = () => sessionStorage.getItem(KEY) ?? localStorage.getItem(KEY)

export function keepToken(token: string, remember: boolean) {
  forgetToken()
  ;(remember ? localStorage : sessionStorage).setItem(KEY, token)
}

export function forgetToken() {
  sessionStorage.removeItem(KEY)
  localStorage.removeItem(KEY)
}

export async function api<T>(token: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(path, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { Authorization: `Bearer ${token}`, ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
    body: body === undefined ? undefined : JSON.stringify(body),
    cache: 'no-store',
  })
  if (response.status === 401) throw new Unauthorized('Jeton refusé.')
  if (!response.ok) {
    const detail = (await response.json().catch(() => ({}))) as { error?: string }
    throw new Error(detail.error ?? `erreur ${response.status}`)
  }
  return (await response.json()) as T
}
