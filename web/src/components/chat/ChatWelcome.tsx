import {ArrowRight, FileText, MessageSquare, UsersRound, Workflow} from 'lucide-react'
import {Button} from '../Primitives'
import {t, useLocale} from '../../i18n'

type Props = {ready:boolean;loading:boolean;error?:string;onRetry:()=>void;onConnect:()=>void}

export function ChatWelcome({ready,loading,error,onRetry,onConnect}:Props) {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 return <div className="conversation-start mx-auto flex w-full max-w-3xl flex-col items-center justify-center px-5 py-6 text-center">
  <span className="mb-5 grid size-12 place-items-center rounded-2xl border border-divider bg-canvas text-ink"><MessageSquare size={29} strokeWidth={1.5}/></span>
  <h1 className="text-3xl font-semibold tracking-tight text-ink">{tr('새 대화를 시작하세요','Start a conversation')}</h1>
  <p className="mt-3 max-w-lg text-sm leading-7 text-muted">{tr('아래에 메시지를 적어 보내세요. AI와 답변 방식은 직접 선택할 수 있습니다.','Write a message below. You can choose the AI and how it responds.')}</p>
  {loading&&<p role="status">{tr('대화 설정을 불러오는 중…','Loading conversation settings…')}</p>}
  {error&&<div className="mt-5 flex flex-wrap items-center gap-3 rounded-xl border border-danger/20 bg-danger-soft p-4 text-sm text-danger" role="alert"><p>{error}</p><Button variant="secondary" onClick={onRetry}>{t('history.retry')}</Button></div>}
  {!ready&&!loading&&!error&&<Button variant="secondary" className="mt-5" onClick={onConnect}>{tr('사용할 AI 연결·선택','Connect or choose an AI')}<ArrowRight size={16}/></Button>}
 </div>
}

export function ConversationStartActions({onGroup}:{onGroup:()=>void}) {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 const actions=[
  {icon:UsersRound,label:tr('그룹 대화','Group conversation'),description:tr('함께할 AI를 고르고 이야기하기','Choose the AIs to talk with'),run:onGroup},
  {icon:Workflow,label:tr('맡긴 작업','Your tasks'),description:tr('진행 상황과 결과 이어보기','Follow progress and results'),run:()=>{window.location.hash='#/tasks'}},
  {icon:FileText,label:t('nav.prompts'),description:tr('시스템 지침과 성격 직접 편집','Edit instructions and personas'),run:()=>{window.location.hash='#/prompts'}},
 ]
 return <div className="flex flex-wrap justify-center gap-1 px-4 pb-4 [&>button]:text-xs sm:[&>button]:text-sm">{actions.map(({icon:Icon,label,description,run})=><Button variant="ghost" key={label} onClick={run} title={description}><Icon size={17}/>{label}</Button>)}</div>
}
