import { Component, type ReactNode } from 'react'
import { t } from '../i18n'
// A running tab retains the entry bundle from the moment it was opened.
// After "npm run build", lazy-loaded page chunks have new hashed URLs, while
// this tab still points at the removed ones. React.lazy alone cannot recover.
const loadedEntry =
  document
    .querySelector<HTMLScriptElement>('script[type="module"][src]')
    ?.getAttribute('src') || ''
const loadedBundle = /\/assets\/[^/?]+\.js(?:\?|$)/.test(loadedEntry)
  ? new URL(loadedEntry, document.baseURI).pathname
  : ''
let reloadScheduled = false
let lastVersionCheck = 0
let versionCheck: Promise<void> | undefined

function reloadUpdatedUi() {
  if (reloadScheduled) return
  // If a new bundle is itself broken, show the recovery button instead of
  // trapping the browser in an endless reload loop.
  const key = 'dominion.ui.autoreload'
  try {
    const last = JSON.parse(sessionStorage.getItem(key) || 'null') as {
      entry: string
      at: number
    } | null
    if (last?.entry === loadedBundle && Date.now() - last.at < 15000) return
    sessionStorage.setItem(
      key,
      JSON.stringify({ entry: loadedBundle, at: Date.now() }),
    )
  } catch {}
  reloadScheduled = true
  window.location.reload()
}

export function checkForUpdatedUi() {
  if (!loadedBundle || reloadScheduled) return Promise.resolve()
  if (versionCheck) return versionCheck
  if (Date.now() - lastVersionCheck < 1500) return Promise.resolve()
  lastVersionCheck = Date.now()
  versionCheck = (async () => {
    try {
      // The Rust server already sends no-cache. Check the actual HTML entry,
      // not an API status flag or a timestamp that changes on every request.
      const response = await fetch('/', {
        cache: 'no-store',
        headers: { Accept: 'text/html' },
        signal: AbortSignal.timeout(2500),
      })
      if (
        !response.ok ||
        !response.headers.get('content-type')?.includes('text/html')
      )
        return
      const next = new DOMParser().parseFromString(
        await response.text(),
        'text/html',
      )
      const nextEntry = next
        .querySelector<HTMLScriptElement>('script[type="module"][src]')
        ?.getAttribute('src')
      if (!nextEntry || !/\/assets\/[^/?]+\.js(?:\?|$)/.test(nextEntry)) return
      const nextBundle = new URL(nextEntry, window.location.origin).pathname
      if (nextBundle !== loadedBundle) reloadUpdatedUi()
    } catch {
      // The API or frontend may be restarting; navigation must remain usable.
    }
  })().finally(() => {
    versionCheck = undefined
  })
  return versionCheck
}

// Vite emits this for missing lazy page assets after a rebuild.
window.addEventListener('vite:preloadError', (event) => {
  event.preventDefault()
  reloadUpdatedUi()
})

export class PageLoadBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false }
  static getDerivedStateFromError() {
    return { failed: true }
  }
  componentDidCatch(error: Error) {
    if (
      /dynamically imported module|importing a module script|loading chunk|failed to fetch.*module|module script/i.test(
        error.message,
      )
    ) {
      reloadUpdatedUi()
    }
  }
  render() {
    if (!this.state.failed) return this.props.children
    return (
      <div
        role="alert"
        className="m-6 max-w-lg rounded-xl border border-[#E7DDCC] bg-white p-6 text-sm text-[#5C544A]"
      >
        <p className="mb-3 font-semibold">{t('shell.loadFailed')}</p>
        <button
          type="button"
          className="rounded-lg bg-brand px-4 py-2 font-semibold text-white"
          onClick={() => window.location.reload()}
        >
          {t('shell.reload')}
        </button>
      </div>
    )
  }
}
