import {AsyncPage} from './AsyncPage'
import {settingsPages, type PageId, type SettingsPage} from '../workspacePages'
import {t, useLocale} from '../i18n'
import {workspaceView} from '../utils/workspaceRouting'

type Props = {
 conversationSettingsRef:(element:HTMLDivElement|null)=>void;page:SettingsPage;mode:'chat'|'rooms'
 onNavigate:(page:PageId,sessionId?:string)=>void
}

/** Management pages live in the same workspace as conversations, with no second sidebar. */
export function WorkspacePage({conversationSettingsRef,page,mode,onNavigate}:Props) {
 const en=useLocale()==='en'
 const replyView=page==='settings'&&workspaceView(window.location.hash,'settings')==='reply'
 const entry=settingsPages[page]
 return <section className="workspace-page flex h-full min-h-0 flex-col bg-surface" aria-label={t(entry.label)}>
  <header className="flex min-h-20 shrink-0 flex-wrap items-baseline gap-x-6 gap-y-2 border-b border-divider px-5 py-5 sm:px-8"><h1 className="text-xl font-semibold tracking-tight text-ink">{t(entry.label)}</h1>{page!=='settings'&&<p className="hidden text-sm text-muted sm:block">{t(entry.description)}</p>}</header>
  <div key={page} className="workspace-page-content min-h-0 flex-1 overflow-y-auto overscroll-contain pb-20 sm:pb-6"><AsyncPage label={t('shell.loadingPage')}>{entry.render(onNavigate)}</AsyncPage>
   {replyView&&<section className="workspace-reply-preferences mx-auto max-w-[1320px] px-5 pb-6 sm:px-7 lg:px-6">
    <p className="mb-4 text-sm text-muted">{mode==='rooms'?(en?'These settings apply to the selected group.':'선택한 그룹의 응답에 적용되는 설정입니다.'):(en?'These settings apply to direct chat.':'개인 채팅의 응답에 적용되는 설정입니다.')}</p>
    <div className="workspace-conversation-settings peer" ref={conversationSettingsRef}/>
    <div className="hidden rounded-xl border border-divider p-4 text-sm text-muted peer-empty:block">
     {mode==='rooms'?(en?'Choose a group before changing its reply settings.':'그룹을 선택한 뒤 답변 설정을 변경할 수 있습니다.'):(en?'Open a chat to load reply settings.':'대화 화면을 열어 답변 설정을 불러오세요.')}
    </div>
   </section>}
  </div>
 </section>
}
