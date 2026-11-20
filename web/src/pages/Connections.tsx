import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {Button,TextInput} from '../components/Primitives'
import {ChannelEditor} from '../components/ChannelEditor'
import {useState} from 'react'
import {Activity,Blocks,BookOpenText,PackagePlus,Sparkles,Server,Check,Globe2,Github,KeyRound,Link2,RadioTower,Settings2,X} from 'lucide-react'
import {del,post} from '../api'
import {Action,FormField,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {t,useLocale} from '../i18n'
import {ChannelDirectory} from '../components/ChannelDirectory'
import {normalizeChannelProbe,type ChannelProbe,type Channel,type ChannelEntry} from '../utils/channelView'
import {ExtensionsPage,type ExtensionTab} from './Extensions'
import {CapabilitiesPage} from './Capabilities'
import {ConnectedAccounts} from '../components/ConnectedAccounts'
import {supportedOAuthAccounts,type OAuthAccountId} from '../utils/oauthAccounts'

type OAuthProvider={id:string;configured:boolean;scope:string;callback?:string;connected?:boolean}
type Connection={id:string;provider:string;displayName:string;scopes:string;hasToken:boolean}

export function ConnectionsPage(){
 useLocale()
 const [section,setSection]=useState<'channels'|'quick-connect'|'accounts'|'capabilities'|ExtensionTab>('channels');
 const [editor,setEditor]=useState<{entry:ChannelEntry;record:any}|null>(null);
 const [error,setError]=useState(''),[info,setInfo]=useState(''),[probe,setProbe]=useState<Record<string,ChannelProbe>>({}),[saving,setSaving]=useState(false);
 const [oauthEditor,setOauthEditor]=useState<{id:OAuthAccountId;clientId:string;clientSecret:string}|null>(null);
 const [oauthSaving,setOauthSaving]=useState(false),[oauthError,setOauthError]=useState('');
 // Each section loads only the data it actually renders. Quick-connect
 // owns its own OAuth polling; extension tabs must not start seven unrelated
 // API reads on every page visit.
 const showChannels=section==='channels';
 const showOauth=section==='accounts'||oauthEditor!==null;
 const registry=useRemote<ChannelEntry[]>(showChannels?'/api/channels/catalog':null,[]);
 const channels=useRemote<Channel[]>(showChannels?'/api/channels':null,[]);
 const oauth=useRemote<OAuthProvider[]>(showOauth?'/api/oauth/providers':null,[]);
 const accounts=useRemote<Connection[]>(showOauth?'/api/oauth/connections':null,[]);
 const agents=useRemote<any[]>(editor?'/api/agents':null,[]);
 const rooms=useRemote<any[]>(showChannels?'/api/rooms':null,[]);
 const providers=useRemote<any[]>(editor?'/api/providers':null,[]);
 const [busyAction,setBusyAction]=useState<string|null>(null);
 const [focusChannelId,setFocusChannelId]=useState<string|null>(null);
 const [refreshing,setRefreshing]=useState(false);
 const openEditor=(entry:ChannelEntry,existing?:Channel)=>{
  if(!entry.available)return;
  const record:any=existing?{...existing}:{name:entry.name,kind:entry.id,enabled:true,syncRoomMessages:entry.mode==='outbound',mentionOnly:true,roomId:'',roomPolicy:'all_or_named',agentId:'',providerId:'',model:''};
  for(const field of entry.fields)if(field.type==='secret')record[field.key]='';
  setEditor({entry,record});setError('');setInfo('');
 };
 const save=async()=>{
  if(!editor||saving)return;
  setSaving(true);setError('');setInfo('');
  let saved:Channel;
  try{
   saved=await post<Channel>('/api/channels',editor.record);
  }catch(e){setError((e as Error).message);setSaving(false);return}
  // Once the write succeeds it is not a failed save merely because a
  // subsequent diagnostic or list refresh cannot reach the remote service.
  if(saved.id){
   setFocusChannelId(saved.id);
   setProbe(previous=>{const next={...previous};delete next[saved.id];return next});
  }
  setEditor(null);
  setInfo(t('conn.dlg.saved',{name:editor.entry.name}));
  setSaving(false);
  try{await Promise.all([channels.reload(),registry.reload()])}
  catch(e){setError((e as Error).message)}
  // Close the loop on setup. Diagnosing never sends a message and its
  // result must not be misrepresented as a confirmed outbound delivery.
  if(saved.id&&saved.enabled)await diagnose(saved.id);
 };
 const diagnose=async(id:string)=>{
  if(busyAction)return;
  setBusyAction('probe:'+id);
  setProbe(previous=>({...previous,[id]:{status:'checking',message:t('conn.dlg.diagnosing')}}));
  try{
   const result=await post<{verified?:boolean;message?:string}>('/api/channels/'+encodeURIComponent(id)+'/diagnose',{});
   const outcome=normalizeChannelProbe(result,t('conn.dlg.verified'),t('conn.dlg.checkNeeded'),new Date().toISOString());
   setProbe(previous=>({...previous,[id]:outcome}));
   await channels.reload();
  }catch(e){
   setProbe(previous=>({...previous,[id]:{
    status:'unverified',message:(e as Error).message,checkedAt:new Date().toISOString()
   }}));
  }finally{setBusyAction(null)}
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
   await Promise.all([oauth.reload(),accounts.reload()])
   setInfo(t('conn.oauthAppSaved'))
   setOauthEditor(null)
  }catch(e){setOauthError((e as Error).message)}finally{setOauthSaving(false)}
 }
 const tabs=[{id:'channels',label:t('conn.tab.channels'),icon:RadioTower},{id:'quick-connect',label:t('conn.tab.quickConnect'),icon:Link2},{id:'accounts',label:t('conn.tab.accounts'),icon:KeyRound},{id:'market',label:'MCP 마켓',icon:Blocks},{id:'installed',label:t('ext.tab.installed'),icon:Server},{id:'skills',label:t('ext.tab.skills'),icon:BookOpenText},{id:'plugins',label:t('ext.tab.plugins'),icon:PackagePlus},{id:'capabilities',label:'역량 마켓',icon:Sparkles}] as const
 const pageHeader=<div className="mb-3 flex flex-wrap items-center justify-between gap-3"><div><h1 className="m-0 text-[19px] font-bold text-[#24211D]">{t('conn.title')}</h1><p className="mt-1 text-[12px] text-[#7C7469]">{t('conn.description')}</p></div>{section==='channels'&&<Button type="button" variant="secondary" disabled={refreshing} onClick={()=>void refreshChannels()}><Activity size={15} className={refreshing?'animate-spin':''}/> {t('conn.refresh')}</Button>}</div>
 const sectionTabs=<nav className="connection-tabs" aria-label={t('conn.navigation')}>{tabs.map(({id,label,icon:Icon},index)=><span key={id} className="inline-flex shrink-0 items-center gap-1">
  {(index===3||index===7)&&<span aria-hidden="true" className="mx-2 h-5 w-px shrink-0 bg-[#DED4C8]"/>}
  <Button variant="bare" aria-current={section===id?'page':undefined} onClick={()=>setSection(id)}><Icon size={15}/>{label}</Button>
 </span>)}</nav>
 if(section!=='channels'&&section!=='accounts'&&section!=='quick-connect')return <div className="mx-auto w-full max-w-[1320px] px-4 py-3 sm:px-6">{pageHeader}{sectionTabs}{section==='capabilities'?<CapabilitiesPage embedded navigate={page=>setSection(page==='extensions'?'market':'channels')}/>:<ExtensionsPage section={section} embedded/>}</div>
 return <div className="mx-auto w-full max-w-[1320px] px-4 py-3 sm:px-6">
  {pageHeader}
  {sectionTabs}
  {section==='quick-connect'&&<ConnectedAccounts onSetupApp={id=>{setOauthError('');setOauthEditor({id,clientId:'',clientSecret:''});setSection('accounts')}}/>}
  {info&&<div role="status" className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{info}</div>}
  {error&&!editor&&<Notice message={error}/>}
  {section==='channels'&&<ChannelDirectory entries={registry.value} bindings={channels.value} focusId={focusChannelId}
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
     {oauth.value.find(item=>item.id===oauthEditor.id)?.configured&&<p role="note" className="rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs leading-5 text-amber-900">{t('conn.oauthAppReconfigure')}</p>}
     {oauthEditor.id==='google'&&<p className="text-xs text-[#7C7469]">{t('conn.oauthGoogleSecret')}</p>}
    <FormField label={t('conn.oauthRedirectUri')}><code className="block break-all rounded-lg bg-[#FAF7F1] p-3 text-xs text-[#4A443C]">{oauth.value.find(item=>item.id===oauthEditor.id)?.callback||''}</code></FormField>
    <FormField label={t('conn.oauthClientId')}><TextInput autoComplete="off" value={oauthEditor.clientId} disabled={oauthSaving} onChange={e=>setOauthEditor(previous=>previous?{...previous,clientId:e.target.value}:previous)}/></FormField>
    <FormField label={t('conn.oauthClientSecret')}><TextInput type="password" autoComplete="new-password" value={oauthEditor.clientSecret} disabled={oauthSaving} onChange={e=>setOauthEditor(previous=>previous?{...previous,clientSecret:e.target.value}:previous)}/></FormField>
    {oauthError&&<Notice message={oauthError}/>}
   </div>
   <div className="flex justify-end gap-2 border-t border-[#EFE9DF] p-5"><Action variant="secondary" disabled={oauthSaving} onClick={()=>setOauthEditor(null)}>{t('conn.cancel')}</Action><Action disabled={oauthSaving||!oauthEditor.clientId.trim()||(oauthEditor.id==='github'&&!oauthEditor.clientSecret.trim())} onClick={()=>void saveOAuthApp()}><Check size={16}/>{t('conn.oauthAppSave')}</Action></div>
  </DialogPanel></DialogBackdrop>}
  {editor&&<ChannelEditor editor={editor} rooms={rooms.value} agents={agents.value}
   providers={providers.value} saving={saving} error={error}
   onChange={record=>setEditor(previous=>previous?{...previous,record}:null)}
   onClose={()=>{if(!saving)setEditor(null)}} onSave={()=>void save()}/>}

 </div>
}
