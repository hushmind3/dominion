import {MessageSquare} from 'lucide-react'
import {Button} from '../Primitives'
import {t, useLocale} from '../../i18n'

type Props = {loading:boolean;error?:string;onRetry:()=>void}

export function ChatWelcome({loading,error,onRetry}:Props) {
 const en=useLocale()==='en'
 const tr=(ko:string,english:string)=>en?english:ko
 return <div className="conversation-start mx-auto flex w-full h-full min-h-48 max-w-3xl flex-col items-center justify-center px-5 py-6 text-center">
  <span className="mb-5 grid size-12 place-items-center rounded-2xl border border-divider bg-canvas text-ink"><MessageSquare size={29} strokeWidth={1.5}/></span>
  <h1 className="text-3xl font-semibold tracking-tight text-ink">{tr('새 대화를 시작하세요','Start a conversation')}</h1>
  <p className="mt-3 max-w-lg text-sm leading-7 text-muted">{tr('아래에 메시지를 적어 보내세요. AI와 답변 방식은 직접 선택할 수 있습니다.','Write a message below. You can choose the AI and how it responds.')}</p>
  {loading&&<p role="status">{tr('대화 설정을 불러오는 중…','Loading conversation settings…')}</p>}
  {error&&<div className="mt-5 flex flex-wrap items-center gap-3 rounded-xl border border-danger/20 bg-danger-soft p-4 text-sm text-danger" role="alert"><p>{error}</p><Button variant="secondary" onClick={onRetry}>{t('history.retry')}</Button></div>}

 </div>
}
