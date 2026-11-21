import {useEffect,useState} from 'react'
import {BookText,Check,Copy,FileText,RefreshCw,Save,ShieldCheck} from 'lucide-react'
import {get,post,type Agent,type ChatSession,koreanReasoningEnabled} from '../../api'
import {Button,TextArea,TextInput,SelectInput} from '../../components/Primitives'
import {FormField,Notice,Panel,Subheading,useRemote} from '../../components/UI'
import {useLocale} from '../../i18n'

type OwnerSettings={id:string;instructions:string;enabled:boolean}
type PromptPreview={
 systemPrompt:string;dynamicContext:string;systemChars:number;systemTokensEstimated:number;
 contextChars:number;contextTokensEstimated:number;agentId?:string|null;agentName?:string|null;
 ownerEnabled:boolean;sampleInput:string;sessionId?:string|null;
}
const tr=(ko:string,en:string,isEnglish:boolean)=>isEnglish?en:ko

/** A view of the real chat system-prompt compiler, not a separate prompt
 * registry. Editable text is saved to the existing owner/agent records. */
export function SystemPromptPanel(){
 const en=useLocale()==='en'
 const agents=useRemote<Agent[]>('/api/agents',[])
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const [agentId,setAgentId]=useState('')
 const [sessionId,setSessionId]=useState('')
 const [owner,setOwner]=useState<OwnerSettings|null>(null)
 const [ownerDraft,setOwnerDraft]=useState('')
 const [ownerEnabled,setOwnerEnabled]=useState(true)
 const [personaDraft,setPersonaDraft]=useState('')
 const [queryDraft,setQueryDraft]=useState('일반적인 질문')
 const [previewQuery,setPreviewQuery]=useState('일반적인 질문')
 const [toolsEnabled,setToolsEnabled]=useState(true)
 const [koreanReasoning,setKoreanReasoning]=useState(koreanReasoningEnabled)
 const [preview,setPreview]=useState<PromptPreview|null>(null)
 const [previewError,setPreviewError]=useState('')
 const [loadError,setLoadError]=useState('')
 const [ownerNotice,setOwnerNotice]=useState('')
 const [personaNotice,setPersonaNotice]=useState('')
 const [busy,setBusy]=useState(false)
 const [loading,setLoading]=useState(true)
 const [previewLoading,setPreviewLoading]=useState(false)
 const [revision,setRevision]=useState(0)
 const selectedAgent=agents.value.find(a=>a.id===agentId)
 const privateSessions=sessions.value.filter(s=>!s.roomId)
 useEffect(()=>{
  let alive=true
  get<OwnerSettings>('/api/settings/assistant').then(settings=>{
   if(!alive)return
   setOwner(settings);setOwnerDraft(settings.instructions||'');setOwnerEnabled(settings.enabled!==false)
  }).catch(error=>{if(alive)setLoadError((error as Error).message)})
   .finally(()=>{if(alive)setLoading(false)})
  return()=>{alive=false}
 },[])
 useEffect(()=>{
  if(!agentId&&agents.value.length){
   const initial=agents.value.find(a=>a.id==='default'||a.name==='기본값')||agents.value[0]
   setAgentId(initial.id)
  }
 },[agentId,agents.value])
 useEffect(()=>{setPersonaDraft(selectedAgent?.systemPrompt||'');setPersonaNotice('')},[agentId,selectedAgent?.systemPrompt])
 useEffect(()=>{
  const controller=new AbortController()
  const params=new URLSearchParams({input:previewQuery,toolsEnabled:String(toolsEnabled),koreanReasoning:String(koreanReasoning)})
  if(agentId)params.set('agentId',agentId)
  if(sessionId)params.set('sessionId',sessionId)
  setPreview(null);setPreviewError('');setPreviewLoading(true)
  get<PromptPreview>('/api/settings/system-prompt/preview?'+params.toString(),controller.signal)
   .then(result=>{if(!controller.signal.aborted)setPreview(result)})
   .catch(error=>{if(!controller.signal.aborted)setPreviewError((error as Error).message)})
   .finally(()=>{if(!controller.signal.aborted)setPreviewLoading(false)})
  return()=>controller.abort()
 },[agentId,sessionId,previewQuery,toolsEnabled,koreanReasoning,revision])
 const changedOwner=owner!==null&&(ownerDraft!==owner.instructions||ownerEnabled!==owner.enabled)
 const changedPersona=selectedAgent!==undefined&&personaDraft!==selectedAgent.systemPrompt
 const saveOwner=async()=>{
  if(busy||!changedOwner)return
  setBusy(true);setLoadError('');setOwnerNotice('')
  try{
   const saved=await post<OwnerSettings>('/api/settings/assistant',{instructions:ownerDraft,enabled:ownerEnabled})
   setOwner(saved);setOwnerDraft(saved.instructions);setOwnerEnabled(saved.enabled!==false)
   setRevision(n=>n+1);setOwnerNotice(tr('공통 지침을 저장했습니다. 다음 실행부터 반영됩니다.','Shared instructions saved for subsequent turns.',en))
  }catch(error){setLoadError((error as Error).message)}
  finally{setBusy(false)}
 }
 const savePersona=async()=>{
  if(!changedPersona||busy||!selectedAgent)return
  setBusy(true);setLoadError('');setPersonaNotice('')
  try{
   const current=(await get<Agent[]>('/api/agents')).find(a=>a.id===agentId)
   if(!current)throw Error(tr('에이전트를 찾을 수 없습니다.','Agent not found.',en))
   await post('/api/agents',{...current,systemPrompt:personaDraft})
   await agents.reload()
   setRevision(n=>n+1)
   setPersonaNotice(tr('에이전트 지침을 저장했습니다.','Agent instructions saved.',en))
  }catch(error){setLoadError((error as Error).message)}
  finally{setBusy(false)}
 }
 const copyPrompt=async()=>{
  if(!preview)return
  try{await navigator.clipboard.writeText(preview.systemPrompt);setPersonaNotice(tr('시스템 프롬프트를 복사했습니다.','System prompt copied.',en))}
  catch(error){setLoadError((error as Error).message)}
 }
 return <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)]">
  <div className="grid min-w-0 gap-5">
   <Panel className="min-w-0 p-5">
    <Subheading title={tr('공통 사용자 지침','Shared owner instructions',en)}/>
    <p className="mb-3 text-xs leading-5 text-[#7C7469]">{tr(
     '실제 assistant_settings/global 레코드입니다. 모든 일반 채팅의 시스템 지침에 반영됩니다. 메모리와는 별도 저장됩니다.',
     'Stored in assistant_settings/global, separately from memories. Included in normal chat system instructions.',en)}</p>
    <label className="mb-3 flex items-center gap-2 text-sm text-[#5C544A]">
     <input type="checkbox" checked={ownerEnabled} disabled={loading||busy} onChange={e=>setOwnerEnabled(e.target.checked)}/>
     {tr('공통 지침 사용','Enable shared instructions',en)}
    </label>
    <FormField label={tr('현재 저장된 공통 지침','Saved shared instructions',en)}>
     <TextArea rows={7} value={ownerDraft} disabled={loading||busy} onChange={e=>setOwnerDraft(e.target.value)}
      aria-label={tr('공통 사용자 지침','Shared owner instructions',en)}/>
    </FormField>
    <div className="mt-3 flex items-center justify-between gap-2">
     <span className="text-xs text-[#8A8176]">{ownerDraft.length.toLocaleString()} / 16,000</span>
     <Button disabled={loading||busy||!changedOwner||ownerDraft.length>16000}
      onClick={()=>void saveOwner()}><Save size={14}/>{tr('공통 지침 저장','Save shared',en)}</Button>
    </div>
    {ownerNotice&&<p role="status" className="mt-2 flex items-center gap-1 text-xs text-emerald-700"><Check size={13}/>{ownerNotice}</p>}
   </Panel>
   <Panel className="min-w-0 p-5">
    <Subheading title={tr('에이전트별 시스템 지침','Agent-specific instructions',en)}/>
    <p className="mb-3 text-xs text-[#7C7469]">{tr('에이전트 설정의 systemPrompt와 같은 필드입니다. 여기서 수정해도 다른 에이전트의 지침은 바뀌지 않습니다.',
    'This is the same systemPrompt field used in Agent settings. Changes affect only the selected agent.',en)}</p>
    <FormField label={tr('에이전트 선택','Select agent',en)}>
     <SelectInput value={agentId} disabled={busy} onChange={e=>setAgentId(e.target.value)}>
      <option value="">{tr('에이전트 미선택 · 공통만','No agent · shared only',en)}</option>
      {agents.value.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}
     </SelectInput>
    </FormField>
    <FormField label={tr('에이전트 지침','Agent instructions',en)}>
     <TextArea rows={9} value={personaDraft} disabled={busy||!selectedAgent}
      onChange={e=>setPersonaDraft(e.target.value)}
      aria-label={tr('에이전트별 시스템 지침','Agent-specific system instructions',en)}/>
    </FormField>
    <div className="mt-3 flex justify-end">
     <Button disabled={busy||!changedPersona} onClick={()=>void savePersona()}>
      <Save size={14}/>{tr('에이전트 지침 저장','Save agent instructions',en)}
     </Button>
    </div>
    {personaNotice&&<p role="status" className="mt-2 flex items-center gap-1 text-xs text-emerald-700"><Check size={13}/>{personaNotice}</p>}
   </Panel>
  </div>
  <div className="grid min-w-0 gap-5">
   <Panel className="min-w-0 p-5">
    <div className="flex flex-wrap items-start justify-between gap-2">
     <div className="flex items-center gap-2"><ShieldCheck size={18} className="text-[#A4583E]"/>
      <h3 className="text-base font-semibold">{tr('실제 프롬프트 조립 미리보기','Compiled prompt preview',en)}</h3>
     </div>
     <Button disabled={!preview} onClick={()=>void copyPrompt()}><Copy size={14}/>{tr('복사','Copy',en)}</Button>
    </div>
    <p className="mt-2 text-xs leading-5 text-[#7C7469]">{tr(
     'Rust 채팅 엔진과 동일한 조립 함수를 호출합니다. 실행 중 추가되는 도구 스키마·대화 기록·일시적 실행 지침은 별도이며, 모델 API를 호출하지 않습니다.',
     'Uses the real Rust chat prompt compiler without calling a model. Per-turn tools, history and temporary overrides are not part of this preview.',en)}</p>
    <div className="mt-4 grid gap-3 sm:grid-cols-2">
     <FormField label={tr('대화 적용 범위','Conversation scope',en)}>
      <SelectInput value={sessionId} onChange={e=>setSessionId(e.target.value)}>
       <option value="">{tr('새 일반 대화 기준','New private chat',en)}</option>
       {privateSessions.map(s=><option key={s.id} value={s.id}>{s.title||s.id}</option>)}
      </SelectInput>
     </FormField>
     <FormField label={tr('미리보기 질문','Sample question',en)}>
      <TextInput maxLength={512} value={queryDraft} onChange={e=>setQueryDraft(e.target.value)}
       aria-label={tr('미리보기 질문','Sample question',en)}/>
     </FormField>
    </div>
    <div className="mt-3 flex flex-wrap items-center gap-3 text-xs text-[#5C544A]">
     <label className="flex items-center gap-1.5"><input type="checkbox" checked={toolsEnabled} onChange={e=>setToolsEnabled(e.target.checked)}/>{tr('도구 활성화 가정','Assume tools enabled',en)}</label>
     <label className="flex items-center gap-1.5"><input type="checkbox" checked={koreanReasoning} onChange={e=>setKoreanReasoning(e.target.checked)}/>{tr('한국어 추론 지침','Korean reasoning instruction',en)}</label>
     <Button variant="secondary" disabled={!queryDraft.trim()||queryDraft.length>512}
      onClick={()=>{setPreviewQuery(queryDraft.trim());setRevision(x=>x+1)}}>
      <RefreshCw size={14}/>{tr('다시 계산','Rebuild',en)}</Button>
    </div>
    {previewLoading&&<p role="status" className="mt-3 text-xs text-[#8A8176]">{tr('실제 조립 함수에서 불러오는 중…','Compiling current prompt…',en)}</p>}
    <Notice message={previewError||loadError||agents.error||sessions.error}/>
    {preview&&<div className="mt-4 space-y-3">
     <div className="grid grid-cols-2 gap-2 text-xs">
      <div className="rounded-lg border border-[#EFE9DF] bg-[#FAF7F1] p-3">
       <span className="block text-[#8A8176]">{tr('시스템 역할 프롬프트','System-role prompt',en)}</span>
       <strong className="mt-1 block">{preview.systemChars.toLocaleString()}{tr('자',' chars',en)} · {tr('약','~',en)} {preview.systemTokensEstimated.toLocaleString()} tokens</strong>
      </div>
      <div className="rounded-lg border border-[#EFE9DF] bg-[#FAF7F1] p-3">
       <span className="block text-[#8A8176]">{tr('추가 기억·학습 문맥','Retrieved context',en)}</span>
       <strong className="mt-1 block">{preview.contextChars.toLocaleString()}{tr('자',' chars',en)} · {tr('약','~',en)} {preview.contextTokensEstimated.toLocaleString()} tokens</strong>
      </div>
     </div>
     <div>
      <h4 className="mb-2 flex items-center gap-2 text-sm font-semibold"><FileText size={14}/>{tr('매 요청에 조립되는 시스템 프롬프트','System prompt compiled per request',en)}</h4>
      <pre className="max-h-[420px] overflow-auto whitespace-pre-wrap break-words rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4 font-mono text-xs leading-6" tabIndex={0}>{preview.systemPrompt}</pre>
     </div>
     <details className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-3">
      <summary className="flex cursor-pointer items-center gap-2 text-sm font-semibold"><BookText size={14}/>{tr('추가 문맥 · 별도 메시지','Retrieved context · separate message',en)}</summary>
      <p className="my-2 text-xs text-[#7C7469]">{tr('선택한 질문·대화·메모리 범위에 따라 달라집니다. 시스템 역할 메시지 자체는 아닙니다.',
       'Depends on the sample question, conversation and memory scope. This is not the system-role message.',en)}</p>
      <pre className="max-h-[340px] overflow-auto whitespace-pre-wrap break-words rounded-lg bg-white p-3 font-mono text-xs leading-5">{preview.dynamicContext||tr('추가 문맥 없음','No retrieved context',en)}</pre>
     </details>
    </div>}
   </Panel>
  </div>
 </div>
}
