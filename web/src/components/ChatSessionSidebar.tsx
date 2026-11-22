import {MessageCircle,Plus,Search,Settings,Trash2,X} from 'lucide-react'
import type {ChatSession} from '../api'
import {Button} from './Primitives'
import {t} from '../i18n'

type Props={
 open:boolean; sessions:ChatSession[]; current?:string; loading:boolean; error:string;
 query:string; onQuery:(value:string)=>void;
 deletingSessionId:string; activeTurnSessionId?:string; streaming:boolean;
 onSelect:(value:ChatSession)=>void; onDelete:(value:ChatSession)=>void;
 onNewChat:()=>void; onSettings:()=>void
}
/** Controlled React view: navigation, query and actions come from Chat state. */
export function ChatSessionSidebar({open,sessions,current,loading,error,query,onQuery,
 deletingSessionId,activeTurnSessionId,streaming,onSelect,onDelete,onNewChat,onSettings}:Props){
 const matches=sessions.filter(s=>!s.roomId && s.title.toLocaleLowerCase('ko').includes(query.trim().toLocaleLowerCase('ko')))
 return <aside aria-label="대화 목록" className={'flex min-h-0 w-64 shrink-0 flex-col overflow-hidden border-r border-divider bg-canvas text-ink '+(open?'absolute inset-y-0 left-0 z-30 shadow-xl md:relative md:z-auto md:shadow-none':'hidden')}>
  <div className="flex items-center justify-between gap-3 px-4 pb-3 pt-5">
   <div className="min-w-0"><span className="block text-sm font-bold text-ink">{t('chat.recent')}</span>
    <small className="mt-0.5 block text-xs text-muted">{loading||error?'—':sessions.filter(s=>!s.roomId).length}</small></div>
   <Button variant="ghost" size="icon" onClick={onNewChat} title={t('chat.newChat')} aria-label={t('chat.newChat')}><Plus size={18}/></Button>
  </div>
  <Button type="button" variant="primary" onClick={onNewChat} className="mx-3 mb-3 !justify-center"><Plus size={15}/>{t('chat.newChat')}</Button>
  <label className="mx-3 mb-3 flex min-h-9 items-center gap-2 rounded-lg border border-divider bg-surface px-2.5 focus-within:border-brand">
   <Search size={15} className="shrink-0 text-muted" aria-hidden="true"/>
   <input type="search" aria-label="대화 목록 검색" placeholder="대화 검색" value={query}
    onChange={event=>onQuery(event.target.value)}
    className="min-w-0 flex-1 border-0 bg-transparent py-1.5 text-sm text-ink placeholder:text-muted focus:outline-none"/>
   {query&&<button type="button" aria-label="검색 지우기" onClick={()=>onQuery('')} className="rounded p-1 text-muted hover:bg-canvas"><X size={13}/></button>}
  </label>
  <nav className="min-h-0 flex-1 overflow-y-auto px-2 pb-3" aria-label="최근 대화">
   {matches.map(session=><div key={session.id} className={'group mb-1 flex min-w-0 items-center rounded-lg border '+(session.id===current?'border-brand/25 bg-brand/10':'border-transparent hover:bg-surface')}>
    <Button variant="bare" type="button" aria-current={session.id===current?'page':undefined}
     onClick={()=>onSelect(session)} title={session.title}
     className={'flex min-h-10 min-w-0 flex-1 items-center !justify-start gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm '+(session.id===current?'font-semibold text-ink':'text-muted')}>
     <MessageCircle size={14} className="shrink-0 text-muted" aria-hidden="true"/>
     <span className="min-w-0 flex-1 truncate">{session.title}</span>
    </Button>
    <Button type="button" variant="ghost" size="icon" title={t('chat.deleteSession')}
     aria-label={session.title+' '+t('chat.deleteSession')}
     disabled={Boolean(deletingSessionId)||(streaming&&activeTurnSessionId===session.id)}
     onClick={()=>onDelete(session)} className="!mr-1 !size-7 shrink-0 text-muted hover:!bg-rose-50 hover:!text-rose-700">
     <Trash2 size={13}/>
    </Button>
   </div>)}
   {!matches.length&&<p role="status" className="px-3 py-6 text-center text-xs leading-5 text-muted">
    {loading?t('chat.sessionsLoading'):error?t('chat.sessionsUnavailable'):query.trim()?'일치하는 대화가 없습니다.':t('chat.first')}
   </p>}
  </nav>
  <div className="shrink-0 border-t border-divider px-3 py-2">
   <Button variant="ghost" onClick={onSettings} className="w-full !justify-start gap-2 text-sm" aria-label={t('nav.settings')}>
    <Settings size={16}/>{t('nav.settings')}
   </Button>
  </div>
 </aside>
}
