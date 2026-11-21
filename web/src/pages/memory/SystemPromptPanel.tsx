import {useEffect,useRef,useState} from 'react'
import {BookText,Check,Copy,FileText,RefreshCw,Save,ShieldCheck,RotateCcw} from 'lucide-react'
import {get,post,type Agent,type ChatSession,koreanReasoningEnabled} from '../../api'
import {Button,TextArea,TextInput,SelectInput} from '../../components/Primitives'
import {FormField,Notice,Panel,Subheading,useRemote} from '../../components/UI'
import {useLocale} from '../../i18n'

type OwnerSettings={id:string;instructions:string;enabled:boolean}
type PromptPreview={
 systemPrompt:string;dynamicContext:string;systemChars:number;systemTokensEstimated:number;
 contextChars:number;contextTokensEstimated:number;agentId?:string|null;agentName?:string|null;
 ownerEnabled:boolean;sampleInput:string;sessionId?:string|null;
 ownOverrideActive:boolean;ownOverrideText?:string|null;effectiveOverrideScope?:'agent'|'global'|null;
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
 const autoSelected=useRef(false)
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
 const [fullDraft,setFullDraft]=useState('')
 const [fullDirty,setFullDirty]=useState(false)
 const [fullNotice,setFullNotice]=useState('')
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
  if(!autoSelected.current&&agents.value.length){
   autoSelected.current=true
   const initial=agents.value.find(a=>a.id==='default'||a.name==='기본값')||agents.value[0]
   setAgentId(initial.id)
  }
 },[agents.value])
 useEffect(()=>{setPersonaDraft(selectedAgent?.systemPrompt||'');setPersonaNotice('')},[agentId,selectedAgent?.systemPrompt])
 useEffect(()=>{setFullDirty(false);setFullNotice('');setFullDraft('')},[agentId,sessionId])
 useEffect(()=>{
  if(preview&&!fullDirty)setFullDraft(preview.ownOverrideText??preview.systemPrompt)
 },[preview,fullDirty])
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
 const saveFullPrompt=async()=>{
  if(busy||!fullDirty||fullDraft.length>32000)return
  setBusy(true);setLoadError('');setFullNotice('')
  try{
   await post('/api/settings/system-prompt/override',{
    ...(agentId?{agentId}:{}),prompt:fullDraft
   })
   setFullDirty(false);setRevision(x=>x+1)
   setFullNotice(tr('전체 시스템 프롬프트를 저장했습니다. 다음 일반 채팅부터 실제 모델 입력에 사용합니다.',
    'Saved. The exact text will be sent for subsequent private-chat turns.',en))
  }catch(error){setLoadError((error as Error).message)}
  finally{setBusy(false)}
 }
 const resetFullPrompt=async()=>{
  if(busy||!preview?.ownOverrideActive)return
  if(!window.confirm(tr('선택한 범위의 직접 편집 프롬프트를 초기화하고 자동 생성 지침으로 돌아가시겠습니까?',
   'Remove this custom prompt and return to the generated instructions?',en)))return
  setBusy(true);setLoadError('');setFullNotice('')
  try{
   await post('/api/settings/system-prompt/override',{
    ...(agentId?{agentId}:{}),prompt:null
   })
   setFullDirty(false);setRevision(x=>x+1)
   setFullNotice(tr('직접 편집 지침을 초기화했습니다.','Custom prompt reset.',en))
  }catch(error){setLoadError((error as Error).message)}
  finally{setBusy(false)}
 }
 const copyPrompt=async()=>{
  if(!preview)return
  try{await navigator.clipboard.writeText(fullDraft);setFullNotice(tr('편집 중인 시스템 프롬프트를 복사했습니다.','Draft system prompt copied.',en))}
  catch(error){setLoadError((error as Error).message)}
 }
 return (
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)]">
   <div className="grid min-w-0 gap-5">
    <Panel className="min-w-0 p-5">
     <Subheading title={tr('전체 시스템 프롬프트','Full system prompt',en)}/>
     <p className="mb-3 text-xs leading-5 text-[#7C7469]">
      {tr('매번 모델로 전송하는 실제 지침입니다. 텍스트를 직접 수정하거나 비워서 저장하면 다음 일반 채팅부터 적용됩니다. 실행 권한과 도구 제한은 Rust가 별도로 관리합니다.',
      'Edit or clear the real system text sent to the model each private-chat turn. Rust separately enforces permissions and tool limits.',en)}
     </p>
     <FormField label={tr('적용할 에이전트','Agent scope',en)}>
      <SelectInput value={agentId} disabled={busy}
       onChange={e=>{
        if(fullDirty&&!window.confirm(tr('저장하지 않은 변경을 버리시겠습니까?','Discard unsaved changes?',en)))return
        setAgentId(e.target.value)
       }}>
       <option value="">{tr('공통 · 모든 일반 채팅','Shared · all private chats',en)}</option>
       {agents.value.map(agent=><option value={agent.id} key={agent.id}>{agent.name}</option>)}
      </SelectInput>
     </FormField>
     <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
      <FormField label={tr('대화 미리보기','Preview conversation',en)}>
       <SelectInput value={sessionId}
        onChange={e=>{
         if(fullDirty&&!window.confirm(tr('저장하지 않은 변경을 버리시겠습니까?','Discard unsaved changes?',en)))return
         setSessionId(e.target.value)
        }}>
        <option value="">{tr('새 일반 대화','New private chat',en)}</option>
        {privateSessions.map(session=><option key={session.id} value={session.id}>{session.title||session.id}</option>)}
       </SelectInput>
      </FormField>
      <FormField label={tr('미리보기 질문','Sample question',en)}>
       <TextInput maxLength={512} value={queryDraft}
        onChange={e=>setQueryDraft(e.target.value)}/>
      </FormField>
     </div>
     <div className="mt-3 flex flex-wrap items-center gap-3 text-xs text-[#5C544A]">
      <label className="flex items-center gap-1.5">
       <input type="checkbox" checked={toolsEnabled} onChange={e=>setToolsEnabled(e.target.checked)}/>
       {tr('도구 활성화 가정','Assume tools available',en)}
      </label>
      <label className="flex items-center gap-1.5">
       <input type="checkbox" checked={koreanReasoning} onChange={e=>setKoreanReasoning(e.target.checked)}/>
       {tr('한국어 추론 지침','Korean reasoning',en)}
      </label>
      <Button variant="secondary" disabled={!queryDraft.trim()||queryDraft.length>512}
       onClick={()=>{setPreviewQuery(queryDraft.trim());setRevision(x=>x+1)}}>
       <RefreshCw size={14}/>{tr('미리보기 갱신','Rebuild preview',en)}
      </Button>
     </div>
     <Notice message={previewError||loadError||agents.error||sessions.error}/>
     {previewLoading&&<p className="mt-3 text-xs text-[#7C7469]">{tr('프롬프트를 불러오는 중…','Loading prompt…',en)}</p>}
     {preview&&(
      <div className="mt-4">
       <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <label className="flex items-center gap-2 text-sm font-semibold">
         <FileText size={15}/>{tr('모델에 전송하는 시스템 텍스트','Model-facing system text',en)}
        </label>
        <Button variant="secondary" onClick={()=>void copyPrompt()}>
         <Copy size={14}/>{tr('복사','Copy',en)}
        </Button>
       </div>
       <TextArea rows={13} className="min-h-[290px] w-full font-mono text-xs leading-6"
        value={fullDraft} disabled={busy}
        aria-label={tr('전체 시스템 프롬프트 직접 편집','Edit full system prompt',en)}
        onChange={e=>{setFullDraft(e.target.value);setFullDirty(true);setFullNotice('')}}/>
       <p className="mt-2 text-xs text-[#7C7469]">
        {tr('저장된 내용이 없으면 자동 생성 지침을 사용합니다. 빈 텍스트로 저장하면 기본 문구가 사라집니다.',
         'Without an override the compiler generates instructions. Saving empty text removes the default text.',en)}
       </p>
       <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
        <span className="text-xs text-[#8A8176]">
         {fullDraft.length.toLocaleString()} / 32,000 · {tr('현재 추정','Current estimate',en)} {preview.systemTokensEstimated.toLocaleString()} tokens
        </span>
        <div className="flex flex-wrap gap-2">
         <Button variant="secondary" disabled={busy||!preview.ownOverrideActive}
          onClick={()=>void resetFullPrompt()}>
          <RotateCcw size={14}/>{tr('기본값 복원','Restore default',en)}
         </Button>
         <Button disabled={busy||!fullDirty||fullDraft.length>32000}
          onClick={()=>void saveFullPrompt()}>
          <Save size={14}/>{tr('전체 프롬프트 저장','Save prompt',en)}
         </Button>
        </div>
       </div>
       {fullNotice&&<p role="status" className="mt-3 text-xs text-emerald-700">{fullNotice}</p>}
       {preview.effectiveOverrideScope==='global'&&agentId!==''&&!preview.ownOverrideActive&&(
        <p className="mt-2 text-xs text-[#7C7469]">{tr('공통 프롬프트가 현재 적용 중입니다. 여기서 저장하면 이 에이전트에만 별도 적용됩니다.',
         'Shared override is active. Saving here creates an agent-specific override.',en)}</p>
       )}
       <details className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-3">
        <summary className="flex cursor-pointer items-center gap-2 text-sm font-semibold">
         <BookText size={14}/>{tr('별도 기억·학습 문맥','Separate retrieved memory and context',en)}
        </summary>
        <p className="my-2 text-xs text-[#7C7469]">
         {preview.contextChars.toLocaleString()}{tr('자',' characters',en)} · {tr('약','~',en)} {preview.contextTokensEstimated.toLocaleString()} tokens
        </p>
        <pre className="max-h-[360px] overflow-auto whitespace-pre-wrap break-words rounded-lg bg-white p-3 font-mono text-xs">
         {preview.dynamicContext||tr('추가 문맥 없음','No retrieved context',en)}
        </pre>
       </details>
       <p className="mt-2 text-xs leading-5 text-[#7C7469]">
        {tr('전체 프롬프트 직접 저장은 기본 정체성·에이전트 지침·공통 지침을 대체합니다. 실행 중 추가되는 도구 스키마·대화 기록은 그대로 분리됩니다. 그룹 대화는 그룹 메뉴에서 관리합니다.',
         'A full override replaces built-in identity, persona and owner instructions in private chat. Tool schemas and conversation history remain separate. Rooms are configured independently.',en)}
       </p>
      </div>
     )}
    </Panel>
   </div>
   <div className="grid min-w-0 gap-5">
    <Panel className="min-w-0 p-5">
     <Subheading title={tr('공통 사용자 지침','Shared owner instructions',en)}/>
     <label className="mb-3 flex items-center gap-2 text-sm text-[#5C544A]">
      <input type="checkbox" checked={ownerEnabled} disabled={loading||busy}
       onChange={e=>setOwnerEnabled(e.target.checked)}/>
      {tr('공통 지침 사용','Enable shared instructions',en)}
     </label>
     <FormField label={tr('공통 지침 내용','Shared instructions',en)}>
      <TextArea rows={7} value={ownerDraft} disabled={loading||busy}
       onChange={e=>setOwnerDraft(e.target.value)}/>
     </FormField>
     <div className="mt-3 flex justify-end">
      <Button disabled={loading||busy||!changedOwner||ownerDraft.length>16000}
       onClick={()=>void saveOwner()}><Save size={14}/>{tr('공통 지침 저장','Save shared instructions',en)}</Button>
     </div>
     {ownerNotice&&<p role="status" className="mt-2 text-xs text-emerald-700">{ownerNotice}</p>}
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title={tr('에이전트별 지침','Agent instructions',en)}/>
     <FormField label={tr('선택한 에이전트 지침','Selected agent instructions',en)}>
      <TextArea rows={9} value={personaDraft} disabled={busy||!selectedAgent}
       onChange={e=>setPersonaDraft(e.target.value)}/>
     </FormField>
     <div className="mt-3 flex justify-end">
      <Button disabled={busy||!changedPersona}
       onClick={()=>void savePersona()}><Save size={14}/>{tr('에이전트 지침 저장','Save agent',en)}</Button>
     </div>
     {personaNotice&&<p role="status" className="mt-2 text-xs text-emerald-700">{personaNotice}</p>}
     <p className="mt-3 text-xs leading-5 text-[#7C7469]">
      {tr('전체 시스템 프롬프트를 직접 저장한 경우에는 이 지침이 자동으로 덧붙지 않습니다.',
       'An active full-prompt override replaces this generated persona section.',en)}
     </p>
    </Panel>
   </div>
  </div>
 )
}
