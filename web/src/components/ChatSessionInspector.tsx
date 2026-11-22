import {X} from 'lucide-react'
import {Button} from './Primitives'

type Props={
 locale:'ko'|'en';onClose:()=>void;title?:string;sessionId?:string;
 agentMode:boolean;providerName?:string;model:string;messageCount:number;
 toolsEnabled:boolean;reasoningEffort:string;totalTokens:number;calls:number;
 cachedTokens:number;pendingTool?:string;streamInterrupted:boolean;
}

/** The owner-facing Chat inspector reads existing session state only.
 * It never fabricates a tool receipt, starts another SSE connection, or
 * issues a request on behalf of the current conversation.
 */
export function ChatSessionInspector({
 locale,onClose,title,sessionId,agentMode,providerName,model,messageCount,
 toolsEnabled,reasoningEffort,totalTokens,calls,cachedTokens,pendingTool,
 streamInterrupted,
}:Props){
 return <aside id="chat-session-inspector" aria-label={locale==='en'?'Session workspace':'대화 정보 워크스페이스'}
   className="chat-inspector flex min-h-0 min-w-0 flex-col border-l border-[#E1DCD3] bg-[#F5F5F1]">
   <header className="flex shrink-0 items-center justify-between border-b border-[#E1DCD3] px-4 py-4">
    <div><span className="text-[10px] font-bold uppercase tracking-[.12em] text-[#A65D42]">DOMINION</span>
     <h2 className="mt-1 text-[15px] font-bold">{locale==='en'?'Session details':'대화 정보'}</h2></div>
    <Button size="icon" variant="ghost" onClick={()=>onClose()} aria-label={locale==='en'?'Close details':'대화 정보 닫기'}><X size={18}/></Button>
   </header>
   <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
    <section className="rounded-xl border border-divider bg-white p-3">
     <h3 className="mb-3 text-[12px] font-semibold">{locale==='en'?'Current session':'현재 대화'}</h3>
     <p className="break-words text-[12px] font-medium">{title|| (locale==='en'?'New conversation':'새 대화')}</p>
     <p className="mt-2 break-all text-[10px] text-muted">{sessionId|| (locale==='en'?'Not saved yet':'아직 저장 전')}</p>
     <span className="mt-3 inline-flex rounded-md bg-brand-soft px-2 py-1 text-[10px] font-semibold text-[#94503B]">{agentMode?(locale==='en'?'Agent mode':'에이전트 실행'):(locale==='en'?'Direct chat':'일반 채팅')}</span>
    </section>
    <section className="rounded-xl border border-divider bg-white p-3">
     <h3 className="mb-3 text-[12px] font-semibold">{locale==='en'?'Model & execution':'모델 · 실행'}</h3>
     <p className="break-words text-[11px] text-[#61584F]">{providerName|| (locale==='en'?'No provider selected':'제공자 선택 전')}</p>
     <p className="mt-1 break-all font-mono text-[11px] font-semibold text-[#30302E]">{model||'—'}</p>
     <dl className="mt-3 grid gap-2 border-t border-[#F0EAE1] pt-3 text-[11px]">
      <div className="flex justify-between gap-2"><dt>{locale==='en'?'Messages':'메시지'}</dt><dd className="font-semibold tabular-nums">{messageCount}</dd></div>
      <div className="flex justify-between gap-2"><dt>{locale==='en'?'Tool access':'도구 접근'}</dt><dd className="font-semibold">{agentMode&&toolsEnabled?(locale==='en'?'Enabled':'허용'):(locale==='en'?'Off':'꺼짐')}</dd></div>
      <div className="flex justify-between gap-2"><dt>{locale==='en'?'Thinking':'추론'}</dt><dd className="font-semibold">{reasoningEffort|| (locale==='en'?'Auto':'자동')}</dd></div>
     </dl>
    </section>
    <section className="rounded-xl border border-divider bg-white p-3">
     <h3 className="mb-3 text-[12px] font-semibold">{locale==='en'?'Session usage':'대화 사용량'}</h3>
     <p className="text-[25px] font-bold tabular-nums text-[#303B35]">{totalTokens.toLocaleString()}<span className="ml-1 text-[11px] font-normal text-[#83796C]">tokens</span></p>
     <dl className="mt-3 grid grid-cols-2 gap-3 text-[10px] text-[#83796C]">
      <div><dt>{locale==='en'?'Model calls':'모델 호출'}</dt><dd className="mt-1 text-[15px] font-bold tabular-nums text-ink">{calls}</dd></div>
      <div><dt>{locale==='en'?'Cached input (known)':'캐시 입력 (확인분)'}</dt><dd className="mt-1 text-[15px] font-bold tabular-nums text-ink">{(Number(cachedTokens)||0).toLocaleString()}</dd></div>
     </dl>
    </section>
    {pendingTool&&<section className="rounded-xl border border-amber-200 bg-amber-50 p-3">
     <h3 className="text-[12px] font-bold text-amber-900">{locale==='en'?'Approval pending':'도구 승인 대기 중'}</h3>
     <p className="mt-1 break-all text-[11px] text-amber-800">{pendingTool}</p>
     <p className="mt-2 text-[10px] text-amber-800">{locale==='en'?'Review the exact call in the composer before deciding.':'작성기 위의 정확한 호출 내용을 검토한 뒤 결정하세요.'}</p>
    </section>}
    {streamInterrupted&&<p role="status" className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-[11px] text-amber-900">{locale==='en'?'The last response was interrupted. Check the saved transcript before retrying.':'마지막 응답이 중단됐습니다. 재요청 전에 저장된 대화 기록을 확인하세요.'}</p>}
   </div>
  </aside>
}
