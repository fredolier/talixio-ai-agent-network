// Access to the master's API. The token stays in the browser: session, or device when asked.

import { t } from './i18n'

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
  if (response.status === 401) throw new Unauthorized(t('Token refused.'))
  if (!response.ok) {
    const detail = (await response.json().catch(() => ({}))) as { error?: string }
    throw new Error(detail.error ?? t('error {status}', { status: response.status }))
  }
  return (await response.json()) as T
}
