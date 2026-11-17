import {useCallback,useEffect,useState} from 'react'
import {CheckCircle2,ExternalLink,Github,Globe2,Link2,RefreshCcw,Unlink} from 'lucide-react'
import {del,openOAuth} from '../api'
import {Action,Notice,Panel,StateTag,useRemote} from './UI'
import {t} from '../i18n'

type Provider={id:string;name:string;configured:boolean;connected:boolean;account?:string|null;scope?:string;credentialSource?:'application'|'local'|'missing'}
type Account={provider:string;displayName:string;hasToken:boolean;scopes:string}

export function ConnectedAccounts(){
 const providers=useRemote<Provider[]>('/api/oauth/providers',[])
 const accounts=useRemote<Account[]>('/api/oauth/connections',[])
 const [busy,setBusy]=useState<string|null>(null)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const refresh=useCallback(()=>{void providers.reload();void accounts.reload()},[providers.reload,accounts.reload])
 useEffect(()=>{window.addEventListener('focus',refresh);return ()=>window.removeEventListener('focus',refresh)},[refresh])
 const connect=async(id:string)=>{
  setBusy(id);setError('');setNotice('')
  try{await openOAuth(id);setNotice(t('conn.quick.authorizeOpened'))}
  catch(e){setError((e as Error).message)}
  finally{setBusy(null)}
 }
 const disconnect=async(id:string)=>{
  if(!window.confirm(t('conn.quick.disconnectConfirm')))return
  setBusy(id);setError('');setNotice('')
  try{await del('/api/oauth/'+encodeURIComponent(id));refresh()}
  catch(e){setError((e as Error).message)}
  finally{setBusy(null)}
 }
 const ordered=[...providers.value].sort((a,b)=>a.id==='github'?-1:b.id==='github'?1:0)
 return <div className="space-y-5">
  <div className="flex flex-wrap items-center justify-between gap-3">
   <div><h2 className="text-[22px] font-bold tracking-tight text-[#24211D]">{t('conn.quick.title')}</h2>
    <p className="mt-2 max-w-2xl text-[13px] leading-6 text-[#7C7469]">{t('conn.quick.description')}</p></div>
   <Action variant="secondary" onClick={refresh}><RefreshCcw size={16}/>{t('conn.quick.refresh')}</Action>
  </div>
  {providers.error&&<Notice message={providers.error}/>}
  {accounts.error&&<Notice message={accounts.error}/>}
  {error&&<Notice message={error}/>}
  {notice&&<p role="status" className="rounded-xl border border-[#D8E8DC] bg-[#F1F8F1] p-3 text-sm text-emerald-800">{notice}</p>}
  <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
   {ordered.map(provider=>{
    const account=accounts.value.find(a=>a.provider===provider.id&&a.hasToken)
    const ready=Boolean(account)
    const Icon=provider.id==='github'?Github:Globe2
    const title=provider.id==='github'?'GitHub':'Google'
    return <Panel key={provider.id} className="flex flex-col gap-4 p-5">
     <div className="flex items-start justify-between gap-3">
      <span className="grid size-12 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={27}/></span>
      <StateTag online={ready}>{ready?t('conn.quick.connected'):provider.configured?t('conn.quick.notConnected'):t('conn.quick.unavailable')}</StateTag>
     </div>
     <div><h3 className="text-lg font-bold text-[#24211D]">{title}</h3>
      <p className="mt-1 text-[13px] leading-6 text-[#7C7469]">{t(provider.id==='github'?'conn.quick.githubDescription':'conn.quick.googleDescription')}</p></div>
     {ready&&<p className="flex items-center gap-2 text-sm font-medium text-emerald-700"><CheckCircle2 size={17}/>{account?.displayName||provider.account||title}</p>}
     {!provider.configured&&<p className="text-xs leading-5 text-amber-800">{t('conn.quick.appSetupMissing')}</p>}
     <div className="mt-auto flex flex-wrap gap-2 border-t border-[#EFE9DF] pt-4">
      <Action disabled={busy!==null} onClick={()=>{void connect(provider.id)}}><Link2 size={17}/>{busy===provider.id?t('conn.quick.opening'):ready?t('conn.quick.reconnect'):t('conn.quick.connect')}<ExternalLink size={14}/></Action>
      {ready&&<Action variant="secondary" disabled={busy!==null} onClick={()=>{void disconnect(provider.id)}}><Unlink size={15}/>{t('conn.quick.disconnect')}</Action>}
     </div>
    </Panel>
   })}
  </div>
  <p className="text-xs leading-6 text-[#8A8176]">{t('conn.quick.renewal')}</p>
 </div>
}
