import {useEffect,useMemo,useState} from 'react'
import {ArrowRight,MessageSquare,Search,UsersRound,X} from 'lucide-react'
import {DialogBackdrop,DialogPanel} from './Dialog'
import {Button,TextInput} from './Primitives'
import {settingsPages,isSettingsPage,type PageId} from '../workspacePages'
import {t,useLocale} from '../i18n'

const hints:Record<string,string>={
 models:'AI LLM API key provider Ollama llama.cpp 모델 키 연결',agents:'agent persona 에이전트 역할',
 connections:'GitHub Google Gmail Discord Telegram Slack MCP 서비스 계정 연결',
 capabilities:'tools shell terminal browser files search 터미널 명령 파일 브라우저 검색 도구',
 extensions:'SDK plugins apps integration tools shell terminal browser files search GitHub Google Gmail Discord Telegram Slack MCP 서비스 계정 연결 터미널 명령 파일 브라우저 검색 도구 확장 플러그인 앱 자동화',
 memory:'memory knowledge learning 기억 지식 학습',activity:'jobs tasks runs approval 작업 실행 승인 기록',
 settings:'preferences backup security 설정 백업 보안',home:'dashboard usage tokens 대시보드 사용량 토큰',
}
export function FeatureSearchButton(){
 const en=useLocale()==='en'
 return <Button variant="ghost" size="icon" title={en?'Find a feature (Ctrl/⌘ K)':'기능 찾기 (Ctrl/⌘ K)'}
  aria-label={en?'Find a feature':'기능 찾기'} aria-haspopup="dialog"
  onClick={()=>window.dispatchEvent(new Event('dominion:feature-search'))}><Search size={17}/></Button>
}
export function FeatureSearch({onNavigate}:{onNavigate:(page:PageId)=>void}){
 const en=useLocale()==='en'
 const [open,setOpen]=useState(false),[query,setQuery]=useState(''),[active,setActive]=useState(0)
 useEffect(()=>{
  const show=()=>{setOpen(true);setQuery('');setActive(0)}
  const key=(event:KeyboardEvent)=>{if(!event.isComposing&&(event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'){event.preventDefault();show()}}
  window.addEventListener('dominion:feature-search',show);window.addEventListener('keydown',key)
  return()=>{window.removeEventListener('dominion:feature-search',show);window.removeEventListener('keydown',key)}
 },[])
 const entries=useMemo(()=>[
  {id:'chat' as PageId,label:t('nav.chat'),description:en?'Chat with a connected model':'연결한 모델과 대화',icon:MessageSquare,hints:'chat 대화 채팅'},
  {id:'rooms' as PageId,label:t('rooms.listTitle'),description:en?'Work with your agents in a shared room':'여러 에이전트와 같은 작업 진행',icon:UsersRound,hints:'group team multiagent 그룹 팀 협업'},
  ...Object.keys(settingsPages).filter(isSettingsPage).filter(id=>!('hidden' in settingsPages[id]&&settingsPages[id].hidden)).map(id=>({id,label:t(settingsPages[id].label),description:t(settingsPages[id].description),icon:settingsPages[id].icon,hints:hints[id]||''})),
 ],[en])
 const terms=query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean)
 const matches=entries.filter(entry=>terms.every(term=>(entry.label+' '+entry.description+' '+entry.hints).toLocaleLowerCase().includes(term)))
 const choose=(page:PageId)=>{setOpen(false);onNavigate(page)}
 if(!open)return null
 return <DialogBackdrop onDismiss={()=>setOpen(false)} style={{zIndex:90}}>
  <DialogPanel aria-label={en?'Find a feature':'기능 찾기'} className="!max-w-2xl">
   <div className="flex items-center gap-2 border-b border-divider p-4"><Search size={19}/>
    <TextInput autoFocus value={query} aria-label={en?'Search features':'기능 검색'} placeholder={en?'Search tools, apps, tasks, models…':'도구, 앱, 작업, 모델 검색…'} className="min-w-0 flex-1"
     role="combobox" aria-expanded={true} aria-controls="feature-results" aria-activedescendant={matches.length?'feature-'+matches[Math.min(active,matches.length-1)].id:undefined}
     onChange={e=>{setQuery(e.target.value);setActive(0)}} onKeyDown={e=>{
      if(e.key==='ArrowDown'){e.preventDefault();setActive(i=>Math.min(i+1,matches.length-1))}
      if(e.key==='ArrowUp'){e.preventDefault();setActive(i=>Math.max(0,i-1))}
      if(e.key==='Enter'&&matches.length){e.preventDefault();choose(matches[Math.max(0,Math.min(active,matches.length-1))].id)}
     }}/>
    <Button variant="ghost" size="icon" aria-label={en?'Close':'닫기'} onClick={()=>setOpen(false)}><X size={18}/></Button>
   </div>
   <div id="feature-results" role="listbox" aria-label={en?'Features':'기능 목록'} className="max-h-[65vh] overflow-y-auto p-2">
    {matches.map((entry,index)=><button id={'feature-'+entry.id} key={entry.id} type="button" role="option" aria-selected={index===Math.max(0,Math.min(active,matches.length-1))}
     className="flex w-full items-center gap-3 rounded-xl p-3 text-left hover:bg-canvas aria-selected:bg-selected"
     onMouseEnter={()=>setActive(index)} onClick={()=>choose(entry.id)}><entry.icon size={19} className="shrink-0 text-muted"/><span className="min-w-0 flex-1"><strong className="block text-sm">{entry.label}</strong><span className="text-xs text-muted">{entry.description}</span></span><ArrowRight size={15}/></button>)}
    {!matches.length&&<p role="status" className="p-5 text-sm text-muted">{en?'No matching features. Try a tool or service name.':'일치하는 기능이 없습니다. 도구나 서비스 이름으로 찾아보세요.'}</p>}
   </div>
  </DialogPanel>
 </DialogBackdrop>
}
