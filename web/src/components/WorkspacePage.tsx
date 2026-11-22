import {AsyncPage} from './AsyncPage'
import {settingsPages, type PageId, type SettingsPage} from '../workspacePages'
import {t, useLocale} from '../i18n'

type Props = {
 conversationSettingsRef:(element:HTMLDivElement|null)=>void;page:SettingsPage
 onNavigate:(page:PageId,sessionId?:string)=>void
}

/** Management pages live in the same workspace as conversations, with no second sidebar. */
export function WorkspacePage({conversationSettingsRef,page,onNavigate}:Props) {
 useLocale()
 const entry=settingsPages[page]
 return <section className="workspace-page flex h-full min-h-0 flex-col bg-surface" aria-label={t(entry.label)}>
  <header className="flex min-h-20 shrink-0 flex-wrap items-baseline gap-x-6 gap-y-2 border-b border-divider px-5 py-5 sm:px-8"><h1 className="text-xl font-semibold tracking-tight text-ink">{t(entry.label)}</h1><p className="hidden text-sm text-muted sm:block">{t(entry.description)}</p></header>
  <div key={page} className="workspace-page-content min-h-0 flex-1 overflow-y-auto overscroll-contain pb-20 sm:pb-6"><AsyncPage label={t('shell.loadingPage')}>{entry.render(onNavigate)}</AsyncPage>
   {page==='settings'&&<section className="workspace-reply-preferences mx-auto max-w-6xl px-4 py-6 sm:px-8"><h2 className="mb-4 text-lg font-semibold">{t('chat.settings')}</h2><div className="workspace-conversation-settings" ref={conversationSettingsRef}/></section>}
  </div>
 </section>
}
