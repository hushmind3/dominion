import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {useState} from 'react'
import {Activity,Blocks,BookOpenText,PackagePlus,Sparkles,Server,Check,ExternalLink,Globe2,Github,KeyRound,Link2,MessageCircle,RadioTower,Settings2,UsersRound,X} from 'lucide-react'
import {del,post} from '../api'
import {Action,FormField,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {t,useLocale} from '../i18n'
import {ChannelDirectory} from '../components/ChannelDirectory'
import {channelDraftMissing,type Channel,type ChannelEntry} from '../utils/channelView'
import {ExtensionsPage,type ExtensionTab} from './Extensions'
import {CapabilitiesPage} from './Capabilities'
import {ConnectedAccounts} from '../components/ConnectedAccounts'
import {supportedOAuthAccounts,type OAuthAccountId} from '../utils/oauthAccounts'

type OAuthProvider={id:string;configured:boolean;scope:string;callback?:string;connected?:boolean}
type Connection={id:string;provider:string;displayName:string;scopes:string;hasToken:boolean}

export function ConnectionsPage(){
 useLocale()
 const registry=useRemote<ChannelEntry[]>('/api/channels/catalog',[]);
 const channels=useRemote<Channel[]>('/api/channels',[]);
 const oauth=useRemote<OAuthProvider[]>('/api/oauth/providers',[]);
 const accounts=useRemote<Connection[]>('/api/oauth/connections',[]);
 const agents=useRemote<any[]>('/api/agents',[]);
 const rooms=useRemote<any[]>('/api/rooms',[]);
 const providers=useRemote<any[]>('/api/providers',[]);
 const [section,setSection]=useState<'channels'|'quick-connect'|'accounts'|'capabilities'|ExtensionTab>('channels');
 const [editor,setEditor]=useState<{entry:ChannelEntry;record:any}|null>(null);
 const [error,setError]=useState(''),[info,setInfo]=useState(''),[probe,setProbe]=useState<Record<string,string>>({}),[saving,setSaving]=useState(false);
 const [oauthEditor,setOauthEditor]=useState<{id:OAuthAccountId;clientId:string;clientSecret:string}|null>(null);
 const [oauthSaving,setOauthSaving]=useState(false),[oauthError,setOauthError]=useState('');
 const [busyAction,setBusyAction]=useState<string|null>(null);
 const [refreshing,setRefreshing]=useState(false);
 const editorMissing=editor?channelDraftMissing(editor.entry,editor.record):[];
 const openEditor=(entry:ChannelEntry,existing?:Channel)=>{
  if(!entry.available)return;
  const record:any=existing?{...existing}:{name:entry.name,kind:entry.id,enabled:false,syncRoomMessages:entry.mode==='outbound',mentionOnly:true,roomId:'',roomPolicy:'all_or_named',agentId:'',providerId:'',model:''};
  for(const field of entry.fields)if(field.type==='secret')record[field.key]='';
  setEditor({entry,record});setError('');setInfo('');
 };
 const save=async()=>{
  if(!editor||saving)return;
  setSaving(true);setError('');
  try{
   await post('/api/channels',editor.record);
   setEditor(null);setInfo(t('conn.dlg.saved',{name:editor.entry.name}));
   await channels.reload();await registry.reload();
  }catch(e){setError((e as Error).message)}finally{setSaving(false)}
 };
 const diagnose=async(id:string)=>{
  if(busyAction)return;
  setBusyAction('probe:'+id);
  setProbe(x=>({...x,[id]:t('conn.dlg.diagnosing')}));
  try{
   const result=await post<{verified?:boolean;message?:string}>('/api/channels/'+encodeURIComponent(id)+'/diagnose',{});
   setProbe(x=>({...x,[id]:result.verified?t('conn.dlg.verified'):result.message||t('conn.dlg.checkNeeded')}));
   await channels.reload();
  }catch(e){setProbe(x=>({...x,[id]:(e as Error).message}))}
  finally{setBusyAction(null)}
 };
 const removeChannel=async(channel:Channel)=>{
  if(busyAction||!window.confirm(t('conn.dlg.removeConfirm',{name:channel.name})))return;
  setBusyAction('delete:'+channel.id);setError('');
  try{
   await del('/api/channels/'+encodeURIComponent(channel.id));
   await channels.reload();
   setProbe(x=>{const next={...x};delete next[channel.id];return next});
   setInfo(t('conn.removed',{name:channel.name}));
  }catch(e){setError((e as Error).message)}
  finally{setBusyAction(null)}
 };
 const refreshChannels=async()=>{
  if(refreshing)return;
  setRefreshing(true);setError('');
  try{await Promise.all([registry.reload(),channels.reload()])}
  finally{setRefreshing(false)}
 };
 const saveOAuthApp=async()=>{
  if(!oauthEditor||oauthSaving)return
  setOauthSaving(true);setOauthError('')
  try{
   await post('/api/oauth/providers/'+encodeURIComponent(oauthEditor.id),{
    clientId:oauthEditor.clientId.trim(),clientSecret:oauthEditor.clientSecret
   })
   await oauth.reload()
   setInfo(t('conn.oauthAppSaved'))
   setOauthEditor(null)
  }catch(e){setOauthError((e as Error).message)}finally{setOauthSaving(false)}
 }
 const tabs=[{id:'channels',label:t('conn.tab.channels'),icon:RadioTower},{id:'quick-connect',label:t('conn.tab.quickConnect'),icon:Link2},{id:'accounts',label:t('conn.tab.accounts'),icon:KeyRound},{id:'market',label:'MCP 마켓',icon:Blocks},{id:'installed',label:t('ext.tab.installed'),icon:Server},{id:'skills',label:t('ext.tab.skills'),icon:BookOpenText},{id:'plugins',label:t('ext.tab.plugins'),icon:PackagePlus},{id:'capabilities',label:'역량 마켓',icon:Sparkles}] as const
 const pageHeader=<div className="mb-3 flex flex-wrap items-center justify-between gap-3"><div><h1 className="m-0 text-[19px] font-bold text-[#24211D]">{t('conn.title')}</h1><p className="mt-1 text-[12px] text-[#7C7469]">{t('conn.description')}</p></div>{section==='channels'&&<Button type="button" variant="secondary" disabled={refreshing} onClick={()=>void refreshChannels()}><Activity size={15} className={refreshing?'animate-spin':''}/> {t('conn.refresh')}</Button>}</div>
 const sectionTabs=<nav className="connection-tabs" aria-label={t('conn.navigation')}>{tabs.map(({id,label,icon:Icon})=><Button key={id} variant="bare" aria-current={section===id?'page':undefined} onClick={()=>setSection(id)}><Icon size={15}/>{label}</Button>)}</nav>
 if(section!=='channels'&&section!=='accounts'&&section!=='quick-connect')return <div className="mx-auto w-full max-w-[1320px] px-4 py-3 sm:px-6">{pageHeader}{sectionTabs}{section==='capabilities'?<CapabilitiesPage embedded navigate={page=>setSection(page==='extensions'?'market':'channels')}/>:<ExtensionsPage section={section} embedded/>}</div>
 return <div className="mx-auto w-full max-w-[1320px] px-4 py-3 sm:px-6">
  {pageHeader}
  {sectionTabs}
  {section==='quick-connect'&&<ConnectedAccounts/>}
  {info&&<div role="status" className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{info}</div>}
  {error&&!editor&&<Notice message={error}/>}
  {section==='channels'&&<ChannelDirectory entries={registry.value} bindings={channels.value}
   rooms={rooms.value} loading={registry.loading||channels.loading} error={registry.error||channels.error}
   probes={probe} busyAction={busyAction} onEdit={openEditor}
   onDiagnose={binding=>void diagnose(binding.id)} onDelete={binding=>void removeChannel(binding)}/>}
  {section==='accounts'&&<>
   <Subheading title={t('conn.accounts')} extra={<Button size="sm" variant="secondary" onClick={()=>setSection('quick-connect')}><Link2 size={14}/>{t('conn.tab.quickConnect')}</Button>}/>
   <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">{supportedOAuthAccounts(oauth.value).map(item=>{const account=accounts.value.find(a=>a.provider===item.id&&a.hasToken);const Icon=item.id==='github'?Github:Globe2;return <Panel className="flex min-w-0 flex-col gap-3 p-5" key={item.id}><div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={28}/></div><h3>{item.id==='github'?'GitHub':'Google'}</h3><p>{item.id==='github'?t('conn.dlg.githubTitle'):t('conn.dlg.googleTitle')}</p><p className="text-[11px] leading-5 text-[#7C7469]">{t(item.id==='github'?'conn.githubCapability':'conn.googleCapability')}</p><small className="break-words text-[10px] text-[#9C9388]">{t('conn.scopeName')}: {account?.scopes||item.scope}</small><div className="integration-status"><StateTag online={Boolean(account)}>{account?t('conn.accountReady',{name:account.displayName}):item.configured?t('conn.oauthReady'):t('conn.oauthNeeded')}</StateTag></div><div className="mt-auto flex flex-wrap items-center gap-2 border-t border-[#EFE9DF] pt-3"><Action variant="secondary" onClick={()=>{setOauthError('');setOauthEditor({id:item.id,clientId:'',clientSecret:''})}}><Settings2 size={15}/>{t('conn.oauthAppSetup')}</Action><Action variant="ghost" onClick={()=>setSection('quick-connect')}><Link2 size={15}/>{account?t('conn.quick.reconnect'):t('conn.quick.connect')}</Action></div></Panel>})}</div>
  </>}
  {oauthEditor&&<DialogBackdrop onDismiss={()=>{if(!oauthSaving)setOauthEditor(null)}}><DialogPanel size="wide" aria-label={t('conn.oauthAppSetup')}>
   <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5"><div><small>OAUTH · {oauthEditor.id.toUpperCase()}</small><h2 className="text-lg font-bold">{t('conn.oauthAppSetup')}</h2></div><Button size="icon" variant="ghost" disabled={oauthSaving} onClick={()=>setOauthEditor(null)} aria-label={t('conn.cancel')}><X size={18}/></Button></div>
   <div className="grid gap-4 p-5">
    <p className="text-xs leading-6 text-[#7C7469]">{t('conn.oauthAppHint')}</p>
    <FormField label={t('conn.oauthRedirectUri')}><code className="block break-all rounded-lg bg-[#FAF7F1] p-3 text-xs text-[#4A443C]">{oauth.value.find(item=>item.id===oauthEditor.id)?.callback||''}</code></FormField>
    <FormField label={t('conn.oauthClientId')}><TextInput autoComplete="off" value={oauthEditor.clientId} disabled={oauthSaving} onChange={e=>setOauthEditor(previous=>previous?{...previous,clientId:e.target.value}:previous)}/></FormField>
    <FormField label={t('conn.oauthClientSecret')}><TextInput type="password" autoComplete="new-password" value={oauthEditor.clientSecret} disabled={oauthSaving} onChange={e=>setOauthEditor(previous=>previous?{...previous,clientSecret:e.target.value}:previous)}/></FormField>
    {oauthError&&<Notice message={oauthError}/>}
   </div>
   <div className="flex justify-end gap-2 border-t border-[#EFE9DF] p-5"><Action variant="secondary" disabled={oauthSaving} onClick={()=>setOauthEditor(null)}>{t('conn.cancel')}</Action><Action disabled={oauthSaving||!oauthEditor.clientId.trim()||!oauthEditor.clientSecret.trim()} onClick={()=>void saveOAuthApp()}><Check size={16}/>{t('conn.oauthAppSave')}</Action></div>
  </DialogPanel></DialogBackdrop>}
  {editor&&<DialogBackdrop onDismiss={()=>{if(!saving)setEditor(null)}}><DialogPanel size="wide" aria-label={editor.entry.name+' '+t('conn.edit')}>
   <div className="sticky top-0 z-10 flex items-center justify-between border-b border-[#EFE9DF] bg-white p-5"><div><small>CHANNEL CONFIGURATION · {editor.entry.id.toUpperCase()}</small><h2>{editor.record.id?t('conn.edit'):t('conn.new')}</h2></div><Button size="icon" variant="ghost" disabled={saving} onClick={()=>setEditor(null)} aria-label={t('conn.cancel')}><X size={18}/></Button></div>
   <div className="grid gap-4 p-5">
    <div className="mb-5 flex items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-5"><div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#C96442]"><MessageCircle size={21}/></div><div><strong>{editor.entry.name}</strong><p>{editor.entry.description}</p></div></div>
    <FormField label={t('conn.name')}><TextInput autoComplete="off" value={editor.record.name||''} onChange={e=>setEditor({...editor,record:{...editor.record,name:e.target.value}})}/></FormField>
    <h3 className="border-t border-[#EFE9DF] pt-4 text-sm font-bold text-[#3A342D]">{t('conn.editorCredentials')}</h3>
    <p className="text-xs leading-5 text-[#7C7469]">{t('conn.editorCredentialsHint')}</p>
    {editor.entry.fields.map(field=><FormField key={field.key} label={field.label+(field.required?' *':'')} hint={field.type==='secret'&&editor.record.id&&editor.record.credentialsPresent?.[field.key]?t('conn.dlg.secretHint'):undefined}><TextInput autoComplete={field.type==='secret'?'new-password':'off'} type={field.type==='secret'?'password':field.type==='url'?'url':'text'} value={editor.record[field.key]||''} placeholder={field.placeholder||''} onChange={e=>setEditor({...editor,record:{...editor.record,[field.key]:e.target.value}})}/></FormField>)}
    <h3 className="border-t border-[#EFE9DF] pt-4 text-sm font-bold text-[#3A342D]">{t('conn.editorRouting')}</h3>
    <FormField label={editor.entry.requiresRoom?t('conn.roomOrAgent'):t('conn.replyMode')}><SelectInput  value={editor.record.roomId||''} onChange={e=>setEditor({...editor,record:{...editor.record,roomId:e.target.value}})}><option value="">{editor.entry.requiresRoom?t('conn.pickRoom'):t('conn.singleAgent')}</option>{rooms.value.map(r=><option value={r.id} key={r.id} disabled={!!editor.entry.maxRoomParticipants&&(r.agentIds?.length||0)>editor.entry.maxRoomParticipants}>{t('conn.dlg.roomOption',{name:r.name,count:r.agentIds?.length||0})}</option>)}</SelectInput></FormField>
    {editor.record.roomId&&<div className="rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-3 text-xs leading-6 text-blue-800"><UsersRound size={19}/><div><strong>{t('conn.roomNote')}</strong><p>{editor.entry.maxRoomParticipants?t('conn.dlg.maxParticipants',{count:editor.entry.maxRoomParticipants}):editor.entry.mode==='outbound'?t('conn.dlg.outboundNote'):t('conn.dlg.roomPolicyNote')}</p></div></div>}
    {editor.record.roomId&&editor.entry.mode!=='outbound'&&<FormField label={t('conn.policy')}><SelectInput  value={editor.record.roomPolicy||'all_or_named'} onChange={e=>setEditor({...editor,record:{...editor.record,roomPolicy:e.target.value}})}><option value="all_or_named">{t('conn.dlg.policyAll')}</option><option value="named_only">{t('conn.dlg.policyNamed')}</option><option value="round_robin">{t('conn.dlg.policyRound')}</option></SelectInput></FormField>}
    {editor.entry.mode!=='outbound'&&<>
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2"><FormField label={t('conn.agent')}><SelectInput  value={editor.record.agentId||''} onChange={e=>setEditor({...editor,record:{...editor.record,agentId:e.target.value}})}><option value="">{t('conn.dlg.pickAgent')}</option>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></FormField><FormField label={t('conn.provider')}><SelectInput  value={editor.record.providerId||''} onChange={e=>setEditor({...editor,record:{...editor.record,providerId:e.target.value}})}><option value="">{t('conn.dlg.agentDefault')}</option>{providers.value.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</SelectInput></FormField></div>
    <FormField label={t('conn.model')}><TextInput  value={editor.record.model||''} onChange={e=>setEditor({...editor,record:{...editor.record,model:e.target.value}})} placeholder={t('conn.dlg.agentDefaultModel')}/></FormField>
    <label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={editor.record.mentionOnly!==false} onChange={e=>setEditor({...editor,record:{...editor.record,mentionOnly:e.target.checked}})}/> {t('conn.mentionOnly')}</label>
    </>}
    <h3 className="border-t border-[#EFE9DF] pt-4 text-sm font-bold text-[#3A342D]">{t('conn.editorBehavior')}</h3>
    {editor.entry.id==='discord'&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.replyMentionBots)} onChange={e=>setEditor({...editor,record:{...editor.record,replyMentionBots:e.target.checked}})}/> {t('conn.dlg.discordMention')}</label>}
    {editor.entry.id==='discord'&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.toolsEnabled)} onChange={e=>setEditor({...editor,record:{...editor.record,toolsEnabled:e.target.checked}})}/> {t('conn.dlg.discordTools')}</label>}
    {editor.record.roomId&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.syncRoomMessages)} onChange={e=>setEditor({...editor,record:{...editor.record,syncRoomMessages:e.target.checked}})}/> {t('conn.dlg.syncRoom')}</label>}
    <label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.enabled)} onChange={e=>setEditor({...editor,record:{...editor.record,enabled:e.target.checked}})}/> {t('conn.enable')}</label>
    {editor.record.webhookPath&&<div className="rounded-xl bg-[#FAF7F1] p-3 font-mono text-xs text-[#4A443C]"><strong>{t('conn.webhook')}</strong><code>{editor.record.webhookPath}</code><small>{t('conn.dlg.webhookNote')}</small></div>}
    {(editor.entry.instructions?.length||0)>0&&<div className="rounded-xl border border-[#E7E0D5] p-4 text-xs leading-6"><strong>{t('conn.instructions')}</strong><ol>{(editor.entry.instructions||[]).map((t,i)=><li key={i}>{t}</li>)}</ol><a href={editor.entry.docs} target="_blank" rel="noreferrer">{t('conn.openDocs')} <ExternalLink size={13}/></a></div>}
   </div>
   {!!editorMissing.length&&<p role="status" className="mx-5 mb-2 rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs text-amber-800">{t('conn.fieldsRequired',{fields:editorMissing.map(field=>field==='roomId'?t('conn.roomOrAgent'):field==='name'?t('conn.name'):field).join(', ')})}</p>}
   {error&&<div className="px-5"><Notice message={error}/></div>}
   <div className="sticky bottom-0 z-10 flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] bg-white p-5"><Action variant="secondary" disabled={saving} onClick={()=>setEditor(null)}>{t('conn.cancel')}</Action><Action onClick={save} disabled={saving||editorMissing.length>0}><Check size={16}/>{t('conn.save')}</Action></div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
