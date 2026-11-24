import {ViewTabs, SearchField, EmptyState} from '../components/ui/WorkspaceUI'
import {workspaceView} from '../utils/workspaceRouting'
import {lazy,Suspense,useEffect,useState} from 'react'
import {Activity, ArrowRight, RefreshCw, Blocks, BookOpenText, Check, ExternalLink, Link2, LoaderCircle, MessageCircle, PackagePlus, Plus, RadioTower, Send, Settings2, ShieldCheck, Trash2, UsersRound, X} from 'lucide-react'
import {DialogBackdrop, DialogPanel} from '../components/Dialog'

import {Button, TextInput, SelectInput, PageContainer} from '../components/Primitives'
import {del, post} from '../api'
import {Action, FormField, Notice, useRemote} from '../components/UI'
import {t, useLocale, type LocaleKey} from '../i18n'
import {ConnectedAccounts} from '../components/ConnectedAccounts'

// Reuse the same MCP registry/installed editor as Tools & MCP.
// Lazy loading avoids starting MCP discovery on ordinary connection visits.
const McpExtensions=lazy(()=>import('./Extensions').then(module=>({default:module.ExtensionsPage})))
import {type ChannelEntry, type ChannelBinding, type ChannelDraft, channelDraftMissing, channelStatusTone} from '../utils/connectionState'

type Editor={entry:ChannelEntry;record:ChannelDraft}

const channelIcons:Record<string,typeof MessageCircle>={
 discord:MessageCircle,telegram:Send,slack:Blocks,whatsapp:MessageCircle,
 signal:ShieldCheck,teams:UsersRound,matrix:Blocks,webhook:Link2,
}

const statusKeys:Record<string,LocaleKey>={
 connected:'conn.status.connected',outbound_ready:'conn.status.outbound_ready',
 sync_disabled:'conn.status.sync_disabled',webhook_ready:'conn.status.webhook_ready',
 starting:'conn.status.starting',reconnecting:'conn.status.reconnecting',
 error:'conn.status.error',disabled:'conn.status.disabled',
 not_started:'conn.status.not_started',stopped:'conn.status.stopped',
}
function statusText(status:string){
 return statusKeys[status]?t(statusKeys[status]):status
}
function ChannelHealth({channel}:{channel:ChannelBinding}){
 const tone=channelStatusTone(channel)
 const colors={
  online:'border-success/20 bg-success-soft/30 text-success',
  pending:'border-warning/20 bg-warning-soft text-warning',
  error:'border-danger/20 bg-danger-soft text-danger',
  disabled:'border-divider bg-canvas text-muted',
 }
 return <span role="status" className={'inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium '+colors[tone]}>
  <span className={'size-1.5 rounded-full '+(tone==='online'?'bg-success':tone==='error'?'bg-danger':tone==='pending'?'bg-warning':'bg-muted')}/>
  {statusText(channel.status)}
 </span>
}

type EditorProps={
 editor:Editor;saving:boolean;error:string;rooms:any[];agents:any[];providers:any[];
 onClose:()=>void;onSave:()=>void;onUpdate:(record:ChannelDraft)=>void;english:boolean;
}
function ChannelEditor({editor,saving,error,rooms,agents,providers,onClose,onSave,onUpdate,english}:EditorProps){
 const {entry,record}=editor
 const set=(key:string,value:unknown)=>onUpdate({...record,[key]:value})
 const missing=channelDraftMissing(entry,record)
 const invalidRoom=Boolean(entry.maxRoomParticipants&&record.roomId&&rooms.some(room=>
  room.id===record.roomId&&(room.agentIds?.length||0)>entry.maxRoomParticipants!))
 const valid=!missing.length&&!invalidRoom
 return <DialogBackdrop onDismiss={()=>{if(!saving)onClose()}}>
  <DialogPanel size="wide">
   <div className="flex items-start justify-between gap-3 border-b border-divider p-5">
    <div className="min-w-0">
     <small className="text-[12px] font-semibold text-muted">{entry.name}</small>
     <h2 className="mt-1 text-lg font-bold text-ink">{record.id?t('conn.edit'):t('conn.new')}</h2>
     <p className="mt-1 text-xs leading-5 text-muted">{entry.description}</p>
    </div>
    <Button size="icon" variant="ghost" disabled={saving} onClick={onClose} aria-label={t('conn.cancel')}><X size={18}/></Button>
   </div>
   <div className="grid min-h-0 flex-1 gap-5 overflow-y-auto p-5">
    <div className="grid gap-3">
     <h3 className="text-[13px] font-bold text-ink">{english?'Connection details':'연결 정보'}</h3>
     <FormField label={t('conn.name')}><TextInput autoFocus value={String(record.name||'')} onChange={e=>set('name',e.target.value)}/></FormField>
     {entry.fields.map(field=><FormField key={field.key}
       label={field.label+(field.required?' *':'')}
       hint={field.type==='secret'&&record.id&&record.credentialsPresent?.[field.key]?t('conn.dlg.secretHint'):undefined}>
      <TextInput type={field.type==='secret'?'password':field.type==='url'?'url':'text'}
       autoComplete={field.type==='secret'?'new-password':undefined}
       value={String(record[field.key]||'')}
       placeholder={field.placeholder||''}
       onChange={e=>set(field.key,e.target.value)}/>
     </FormField>)}
    </div>
    <div className="grid gap-3 border-t border-divider pt-4">
     <h3 className="text-[13px] font-bold text-ink">{english?'AI and conversation':'연결할 AI와 대화'}</h3>
     <FormField label={entry.requiresRoom?t('conn.roomOrAgent'):t('conn.replyMode')}>
      <SelectInput value={record.roomId||''} onChange={e=>set('roomId',e.target.value)}>
       <option value="">{entry.requiresRoom?t('conn.pickRoom'):t('conn.singleAgent')}</option>
       {rooms.map(room=><option value={room.id} key={room.id}
        disabled={!!entry.maxRoomParticipants&&(room.agentIds?.length||0)>entry.maxRoomParticipants}>
        {t('conn.dlg.roomOption',{name:room.name,count:room.agentIds?.length||0})}
       </option>)}
      </SelectInput>
     </FormField>
     {record.roomId&&<div className="flex gap-2 rounded-xl border border-divider bg-canvas p-3 text-xs leading-5 text-muted">
      <UsersRound size={16} className="shrink-0"/><div><strong>{t('conn.roomNote')}</strong><p>{entry.maxRoomParticipants?t('conn.dlg.maxParticipants',{count:entry.maxRoomParticipants}):entry.mode==='outbound'?t('conn.dlg.outboundNote'):t('conn.dlg.roomPolicyNote')}</p></div>
     </div>}
     {record.roomId&&entry.mode!=='outbound'&&<FormField label={t('conn.policy')}>
      <SelectInput value={record.roomPolicy||'all_or_named'} onChange={e=>set('roomPolicy',e.target.value)}>
       <option value="all_or_named">{t('conn.dlg.policyAll')}</option>
       <option value="named_only">{t('conn.dlg.policyNamed')}</option>
       <option value="round_robin">{t('conn.dlg.policyRound')}</option>
      </SelectInput>
     </FormField>}
     {entry.mode!=='outbound'&&<>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
       <FormField label={t('conn.agent')}><SelectInput value={record.agentId||''} onChange={e=>set('agentId',e.target.value)}>
        <option value="">{t('conn.dlg.pickAgent')}</option>{agents.map(agent=><option value={agent.id} key={agent.id}>{agent.name}</option>)}
       </SelectInput></FormField>
       <FormField label={t('conn.provider')}><SelectInput value={record.providerId||''} onChange={e=>set('providerId',e.target.value)}>
        <option value="">{t('conn.dlg.agentDefault')}</option>{providers.map(provider=><option value={provider.id} key={provider.id}>{provider.name}</option>)}
       </SelectInput></FormField>
      </div>
      <FormField label={t('conn.model')}><TextInput value={record.model||''} placeholder={t('conn.dlg.agentDefaultModel')} onChange={e=>set('model',e.target.value)}/></FormField>
      <label className="flex items-center gap-2 text-[12px] text-ink">
       <TextInput type="checkbox" checked={record.mentionOnly!==false} onChange={e=>set('mentionOnly',e.target.checked)}/>{t('conn.mentionOnly')}
      </label>
     </>}
    </div>
    <div className="grid gap-3 border-t border-divider pt-4">
     <h3 className="text-[13px] font-bold text-ink">{english?'Delivery options':'전송 옵션'}</h3>
     {entry.id==='discord'&&<>
      <label className="flex items-center gap-2 text-[12px] text-ink">
       <TextInput type="checkbox" checked={Boolean(record.replyMentionBots)} onChange={e=>set('replyMentionBots',e.target.checked)}/>{t('conn.dlg.discordMention')}
      </label>
      <label className="flex items-center gap-2 text-[12px] text-ink">
       <TextInput type="checkbox" checked={Boolean(record.toolsEnabled)} onChange={e=>set('toolsEnabled',e.target.checked)}/>{t('conn.dlg.discordTools')}
      </label>
     </>}
     {record.roomId&&<label className="flex items-center gap-2 text-[12px] text-ink">
      <TextInput type="checkbox" checked={Boolean(record.syncRoomMessages)} onChange={e=>set('syncRoomMessages',e.target.checked)}/>{t('conn.dlg.syncRoom')}
     </label>}
     <label className="flex items-center gap-2 rounded-xl border border-divider bg-canvas p-3 text-[12px] font-semibold text-ink">
      <TextInput type="checkbox" checked={Boolean(record.enabled)} onChange={e=>set('enabled',e.target.checked)}/>{t('conn.enable')}
     </label>
    </div>
    {record.webhookPath&&<div className="rounded-xl bg-canvas p-3 text-xs text-ink">
     <strong className="block">{t('conn.webhook')}</strong><code className="block break-all">{record.webhookPath}</code>
     <small>{t('conn.dlg.webhookNote')}</small>
    </div>}
    {entry.instructions?.length?<details className="rounded-xl border border-divider p-4 text-xs leading-6">
     <summary className="cursor-pointer font-bold">{t('conn.instructions')}</summary>
     <ol className="mt-2 list-decimal space-y-1 pl-5">{entry.instructions.map((line,index)=><li key={index}>{line}</li>)}</ol>
     {entry.docs&&<a href={entry.docs} target="_blank" rel="noopener noreferrer" className="mt-2 inline-flex items-center gap-1 font-semibold text-brand-hover">{t('conn.openDocs')}<ExternalLink size={13}/></a>}
    </details>:null}
   </div>
   {error&&<div className="px-5"><Notice message={error}/></div>}
   <div className="flex flex-wrap items-center justify-between gap-2 border-t border-divider px-5 py-4">
    <span role="status" className="text-[11px] text-muted">
     {missing.length?(english?'Required fields missing':'필수 항목을 입력하세요'):invalidRoom?(english?'Room capacity exceeded':'회의실 참여 인원 초과'):''}
    </span>
    <div className="flex gap-2">
     <Action variant="secondary" onClick={onClose} disabled={saving}>{t('conn.cancel')}</Action>
     <Action onClick={onSave} disabled={saving||!valid}>
      {saving?<LoaderCircle size={16} className="animate-spin"/>:<Check size={16}/>}
      {t('conn.save')}
     </Action>
    </div>
   </div>
  </DialogPanel>
 </DialogBackdrop>
}

type ConnectionView='saved'|'add'|'accounts'|'mcp'|'registry'|'plugins'|'skills'
function connectionViewFromHash():ConnectionView{
 const view=workspaceView(window.location.hash,'connections')
 return view==='add'||view==='accounts'||view==='mcp'||view==='registry'||view==='plugins'||view==='skills'?view:'saved'
}

export function ConnectionsPage(){
 const locale=useLocale()
 const english=locale==='en'
 const registry=useRemote<ChannelEntry[]>('/api/channels/catalog',[])
 const channels=useRemote<ChannelBinding[]>('/api/channels',[])
 const agents=useRemote<any[]>('/api/agents',[])
 const rooms=useRemote<any[]>('/api/rooms',[])
 const providers=useRemote<any[]>('/api/providers',[])
 const [view,setView]=useState<ConnectionView>(connectionViewFromHash)
 const legacyRuntime=useRemote<{available:boolean}>('/api/mcp/runtime',{available:false})
 useEffect(()=>{
  const sync=()=>setView(connectionViewFromHash())
  window.addEventListener('hashchange',sync);window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 const choose=(next:ConnectionView)=>{
  setView(next)
  window.location.hash=next==='saved'?'#/connections':'#/connections?view='+next
 }
 const [query,setQuery]=useState('')
 const [editor,setEditor]=useState<Editor|null>(null)
 const [error,setError]=useState('')
 const [info,setInfo]=useState('')
 const [probe,setProbe]=useState<Record<string,string>>({})
 const [probing,setProbing]=useState<string|null>(null)
 const [deleting,setDeleting]=useState<string|null>(null)
 const [saving,setSaving]=useState(false)
 const openEditor=(entry:ChannelEntry,existing?:ChannelBinding)=>{
  if(!entry.available)return
  const record:ChannelDraft=existing?{...existing}:{
   name:entry.name,kind:entry.id,enabled:true,
   syncRoomMessages:entry.mode==='outbound',mentionOnly:true,
   roomId:'',roomPolicy:'all_or_named',agentId:'',providerId:'',model:'',
  }
  for(const field of entry.fields)if(field.type==='secret')record[field.key]=''
  setEditor({entry,record});setError('');setInfo('')
 }
 const save=async()=>{
  if(!editor||saving||channelDraftMissing(editor.entry,editor.record).length)return
  setSaving(true);setError('')
  try{
   await post('/api/channels',editor.record)
   setEditor(null)
   setInfo(t('conn.dlg.saved',{name:editor.entry.name}))
   await Promise.all([channels.reload(),registry.reload()])
  }catch(e){setError((e as Error).message)}
  finally{setSaving(false)}
 }
 const diagnose=async(id:string)=>{
  if(probing)return
  setProbing(id)
  setProbe(current=>({...current,[id]:t('conn.dlg.diagnosing')}))
  try{
   const result=await post<{verified:boolean;message?:string}>('/api/channels/'+encodeURIComponent(id)+'/diagnose',{})
   setProbe(current=>({...current,[id]:result.verified?t('conn.dlg.verified'):result.message||t('conn.dlg.checkNeeded')}))
   await channels.reload()
  }catch(e){setProbe(current=>({...current,[id]:(e as Error).message}))}
  finally{setProbing(null)}
 }
 const remove=async(channel:ChannelBinding)=>{
  if(deleting||!window.confirm(t('conn.dlg.removeConfirm',{name:channel.name})))return
  setDeleting(channel.id);setError('');setInfo('')
  try{
   await del('/api/channels/'+encodeURIComponent(channel.id))
   setProbe(current=>{const next={...current};delete next[channel.id];return next})
   await channels.reload()
  }catch(e){setError((e as Error).message)}
  finally{setDeleting(null)}
 }
 const visibleBindings=channels.value.filter(channel=>(channel.name+' '+channel.kind).toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
 const choices=registry.value.filter(entry=>(entry.name+' '+entry.id).toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())).sort((a,b)=>a.name.localeCompare(b.name))
 const tr=(ko:string,en:string)=>english?en:ko
 return <PageContainer>
  <ViewTabs value={view} onChange={choose} label={tr('서비스 연결 보기','Connection views')} options={[{id:'saved',label:tr('내 연결','Your connections'),count:channels.value.length},{id:'add',label:tr('연결 추가','Add a connection')},{id:'accounts',label:tr('앱 계정','App accounts')},...(legacyRuntime.value.available||view==='mcp'?[{id:'mcp' as const,label:tr('MCP 연결','MCP connections'),icon:Blocks}]:[]),{id:'registry' as const,label:tr('MCP 레지스트리','MCP registry'),icon:RadioTower},{id:'plugins',label:tr('확장 프로그램','Extensions'),icon:PackagePlus},{id:'skills',label:tr('스킬','Skills'),icon:BookOpenText}]}/>

  <Notice message={error||(view==='saved'||view==='add'?registry.error||channels.error:'')} onRetry={()=>{void registry.reload();void channels.reload()}}/>
  {info&&<p className="mb-4 flex items-center gap-2 rounded-xl border border-success/20 bg-success-soft/30 p-4 text-sm text-success" role="status"><Check size={16}/>{info}</p>}
  {view==='accounts'?<ConnectedAccounts/>:
   (view==='mcp'||view==='registry'||view==='plugins'||view==='skills')?
    <Suspense fallback={<p className="p-5 text-sm text-muted">{tr('확장 관리 화면 여는 중…','Loading extensions…')}</p>}>
     <McpExtensions section={view==='mcp'?'installed':view==='registry'?'market':view} embedded
      onMcpRegistered={()=>choose('mcp')}/>
    </Suspense>:<>
   <div className="mb-6 flex flex-wrap items-center gap-3 [&>label]:max-w-xl"><SearchField value={query} onChange={setQuery} label={tr('서비스 연결 검색','Search connections')} placeholder={tr('서비스 이름으로 찾기','Search by service name')}/>
    {view==='saved'&&<Button variant="primary" onClick={()=>{setQuery('');choose('add')}}><Plus size={17}/>{tr('새 연결','New connection')}</Button>}
   </div>
   {view==='saved'&&<>
    {!channels.loading&&!channels.error&&!channels.value.length?<EmptyState icon={RadioTower} title={tr('연결한 서비스가 없습니다','No connected services yet')} description={tr('사용할 메신저나 앱을 직접 선택해 연결하세요. AI와 대화하거나 답변을 보내는 데 사용할 수 있습니다.','Choose a messenger or app to chat with your AI or receive its replies.')} action={<Button variant="primary" onClick={()=>choose('add')}>{tr('연결할 서비스 선택','Choose a service')}<ArrowRight size={17}/></Button>}/>:<div className="grid gap-4">
     {visibleBindings.map(channel=>{const entry=registry.value.find(item=>item.id.toLocaleLowerCase()===channel.kind.toLocaleLowerCase());const Icon=channelIcons[channel.kind.toLocaleLowerCase()]||MessageCircle;return <article key={channel.id} className="flex min-w-0 flex-wrap items-start gap-4 rounded-2xl border border-divider bg-surface p-5 sm:p-6">
      <span className="grid size-12 shrink-0 place-items-center rounded-xl border border-divider bg-brand-soft text-brand"><Icon size={24}/></span><div className="min-w-0 flex-1 [&>h2]:text-base [&>h2]:font-semibold [&>p]:mt-1.5 [&>p]:text-sm [&>p]:leading-7 [&>p]:text-muted [&>[role=status]]:mt-2"><h2>{channel.name}</h2><p>{entry?.name||channel.kind}{channel.roomId?' · '+(rooms.value.find(room=>room.id===channel.roomId)?.name||tr('그룹 확인 필요','Review group')):''}</p><ChannelHealth channel={channel}/>{channel.runtimeError&&<p className="!text-danger" role="alert">{channel.runtimeError}</p>}{probe[channel.id]&&<p role="status">{probe[channel.id]}</p>}</div>
      <div className="flex w-full flex-wrap justify-end gap-2 lg:w-auto"><Button variant="secondary" disabled={probing===channel.id} onClick={()=>void diagnose(channel.id)}>{probing===channel.id?<LoaderCircle size={16} className="animate-spin"/>:<Activity size={16}/>} {tr('연결 확인','Check connection')}</Button>{entry&&<Button variant="ghost" onClick={()=>openEditor(entry,channel)}><Settings2 size={16}/>{tr('수정','Edit')}</Button>}<Button variant="ghost" size="icon" disabled={deleting===channel.id} onClick={()=>void remove(channel)} aria-label={tr(channel.name+' 연결 삭제','Remove '+channel.name)}><Trash2 size={16}/></Button></div>
     </article>})}
     {!channels.loading&&!visibleBindings.length&&<p className="p-10 text-center text-sm text-muted">{tr('검색 결과가 없습니다.','No matching connections.')}</p>}
    </div>}
    {channels.value.length>0&&<Button variant="ghost" className="mt-4" onClick={()=>void channels.reload()}><RefreshCw size={16}/>{tr('상태 새로고침','Refresh status')}</Button>}
   </>}
   {view==='add'&&<section className="[&>header]:mb-6 [&_h2]:text-xl [&_h2]:font-semibold [&_header>p]:mt-2 [&_header>p]:text-sm [&_header>p]:leading-7 [&_header>p]:text-muted"><header><h2>{tr('어디에서 사용하고 싶으세요?','Where would you like to use it?')}</h2><p>{tr('서비스를 선택하면 필요한 정보만 순서대로 입력할 수 있습니다.','Choose a service, then enter the required details step by step.')}</p></header>
    <div className="grid grid-cols-1 gap-3 md:grid-cols-2 [&>button]:flex [&>button]:items-center [&>button]:gap-4 [&>button]:rounded-2xl [&>button]:border [&>button]:border-divider [&>button]:bg-surface [&>button]:p-5 [&>button]:text-left [&>button:hover]:border-brand [&>button>span:nth-child(2)]:min-w-0 [&>button>span:nth-child(2)]:flex-1 [&_strong]:text-base [&_strong]:font-semibold [&_small]:mt-1.5 [&_small]:block [&_small]:text-sm [&_small]:leading-7 [&_small]:text-muted">{choices.map(entry=>{const Icon=channelIcons[entry.id.toLocaleLowerCase()]||MessageCircle;return <Button key={entry.id} variant="bare" disabled={!entry.available} onClick={()=>openEditor(entry)}><span className="grid size-12 shrink-0 place-items-center rounded-xl border border-divider bg-brand-soft text-brand"><Icon size={23}/></span><span><strong>{entry.name}</strong><small>{entry.available?(entry.mode==='outbound'?tr('AI 답변을 이곳으로 받기','Receive AI replies here'):tr('이 서비스에서 AI와 대화하기','Chat with your AI here')):tr('준비 중','Not available yet')}</small></span><ArrowRight size={18}/></Button>})}</div>
   </section>}
  </>}
  {editor&&<ChannelEditor editor={editor} saving={saving} error={error} rooms={rooms.value} agents={agents.value} providers={providers.value} english={english} onClose={()=>setEditor(null)} onSave={()=>void save()} onUpdate={record=>setEditor(current=>current?{...current,record}:current)}/>}
 </PageContainer>
}
