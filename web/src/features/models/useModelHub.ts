import {useEffect,useMemo,useRef,useState} from 'react'
import {Provider,del,get,post} from '../../api'
import {refresh} from '../../dataRefresh'
import {useRemote} from '../../hooks/useRemote'
import {useProviderModels} from '../../hooks/useProviderModels'
import {t,useLocale} from '../../i18n'
import type {CliAuthStatus} from '../../components/CliAuthProgress'
import {opencodeGoPreset, providerPresets, updatedProviderEndpoint, type PresetCategory, type ProviderPreset, type CliCredential, type CatalogProvider, type CatalogResponse, type FreeCatalog, type Tab} from './catalog'

export function useModelHub({inline=false}:{inline?:boolean}={}){
 useLocale()
 const providers=useRemote<Provider[]>('/api/providers',[])
 const cli=useRemote<CliCredential[]>(inline?null:'/api/providers/cli',[])
 const [counts,setCounts]=useState<Record<string,{list:string[];error?:string;info?:string;busy?:boolean}>>({})
 const [error,setError]=useState(''),[saving,setSaving]=useState(false)
 const [savedId,setSavedId]=useState(''),[removedName,setRemovedName]=useState('')
 const [integration,setIntegration]=useState<ProviderPreset['integration']>()
 const [freePreset,setFreePreset]=useState(false)
 const providerList=useRef<HTMLDetailsElement>(null)
 const [modal,setModal]=useState(false),[presetMode,setPresetMode]=useState(false)
 const [edit,setEdit]=useState<Provider|null>(null)
 const [tab,setTab]=useState<Tab>('providers'),[category,setCategory]=useState<'all'|PresetCategory>('all'),[query,setQuery]=useState('')
 const [catalogSort,setCatalogSort]=useState<'name'|'models'>('name'),[catalogLimit,setCatalogLimit]=useState(60)
 const [browseProviderId,setBrowseProviderId]=useState('')
 const catalog=useRemote<CatalogResponse>(tab==='catalog'?'/api/providers/catalog':null,{providers:[]})
 const [loginPending,setLoginPending]=useState(false),[loginUrl,setLoginUrl]=useState(''),[loginMessage,setLoginMessage]=useState(''),[deviceCode,setDeviceCode]=useState<{userCode:string;verificationUrl:string}|null>(null)
 const freeCatalog=useRemote<FreeCatalog>(inline?null:'/api/providers/free-catalog',{providers:[]})
 const [freeBusy,setFreeBusy]=useState(false)
 const [oauthBusy,setOauthBusy]=useState('')
 const [cliAuth,setCliAuth]=useState<CliAuthStatus|null>(null)
 const cliLoginPopup=useRef<Window|null>(null)
 const cliLoginPopupUrl=useRef('')
 const [openCodeAuthProvider,setOpenCodeAuthProvider]=useState('openai')
 const [cliModelCatalog,setCliModelCatalog]=useState<Record<string,string[]>>({})
 const [cliModelSelection,setCliModelSelection]=useState<Record<string,string>>({})
 const [cliModelBusy,setCliModelBusy]=useState('')
 const cliModelRequested=useRef(new Set<string>())
 const fetchCliModels=async(source:string)=>{
  const providerId='cli-'+source
  setCliModelBusy(source)
  try{
   const rows=await get<{id:string}[]>('/api/providers/'+providerId+'/models')
   const available=rows.map(r=>r.id).filter(Boolean)
   const defaultChoice=source==='gemini'?'auto':'default'
   const options=[defaultChoice,...available.filter(model=>model!==defaultChoice)]
   setCliModelCatalog(old=>({...old,[source]:options}))
   setCliModelSelection(old=>({...old,[source]:old[source]&&options.includes(old[source])?old[source]:defaultChoice}))
  }catch(e){setError((e as Error).message)}
  finally{setCliModelBusy('')}
 }
 useEffect(()=>{
  if(tab!=='oauth'&&tab!=='connect')return
  for(const item of cli.value){
   if(!item.connected||!['claude','gemini','opencode'].includes(item.id)||cliModelRequested.current.has(item.id))continue
   cliModelRequested.current.add(item.id)
   void fetchCliModels(item.id)
  }
 },[tab,cli.value])
 const saveCliModel=async(source:string)=>{
  const model=cliModelSelection[source]
  if(!model)return
  setCliModelBusy(source)
  try{
   await post('/api/providers/cli/'+source+'/model',{model})
   await Promise.all([providers.reload(),cli.reload()])
   setLoginMessage('사용할 모델을 저장했습니다. 이제 채팅에서 바로 선택할 수 있습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setCliModelBusy('')}
 }
 const [name,setName]=useState(''),[baseUrl,setBaseUrl]=useState(''),[kind,setKind]=useState('openai'),[apiKey,setApiKey]=useState(''),[local,setLocal]=useState(false),[modelId,setModelId]=useState(''),[defaultModel,setDefaultModel]=useState(''),[reflection,setReflection]=useState(false),[rpm,setRpm]=useState(''),[accountId,setAccountId]=useState(''),[testBusy,setTestBusy]=useState(false),[testResult,setTestResult]=useState<{ok:boolean;message:string}|null>(null)
 const [defaultEffort,setDefaultEffort]=useState('')
 const [vision,setVision]=useState(false)
 const editModels=useProviderModels(modal&&edit?edit.id:'')
 const probe=async(p:Provider)=>{
  setCounts(old=>({...old,[p.id]:{list:[],busy:true}}))
  try{const list=await get<{id:string;catalogStale?:boolean;catalogSource?:string}[]>('/api/providers/'+p.id+'/models');setCounts(old=>({...old,[p.id]:{list:list.map(m=>m.id),error:list.some(m=>m.catalogStale)?'최근 저장된 목록입니다. 실시간 Codex 조회가 실패했습니다.':undefined,info:list.some(m=>m.catalogSource==='models.dev')?'API 키 없이 공개 목록을 조회했습니다. 실제 채팅 사용에는 인증이 필요할 수 있습니다.':undefined}}))}
  catch(e){setCounts(old=>({...old,[p.id]:{list:[],error:(e as Error).message}}))}
 }
 const open=(p?:Provider)=>{setEdit(p||null);setIntegration(p?.integration==='freellmapi'?'freellmapi':p?.integration==='opencode-go'?'opencode-go':undefined);setFreePreset(false);setPresetMode(false);setName(p?.name||'');setBaseUrl(p?(updatedProviderEndpoint(p)||p.baseUrl):'');setKind(p?.kind||'openai');setApiKey('');setLocal(Boolean(p?.local));setModelId(p?.reflectionModel||(p?.reflection??p?.learning?p?.model:'')||'');setDefaultModel(p?.model||'');setDefaultEffort(String(p?.reasoningEffort||''));setReflection(Boolean(p?.reflection ?? p?.learning));setRpm(p?.requestsPerMinute?String(p.requestsPerMinute):'');setAccountId(p?.accountId||'');setVision(p?.capabilities?.vision===true);setTestResult(null);setError('');setModal(true)}
 const openPreset=(preset:ProviderPreset)=>{setEdit(null);setIntegration(preset.integration);setFreePreset(Boolean(preset.free&&preset.auth==='none'));setPresetMode(true);setName(preset.name);setBaseUrl(preset.baseUrl);setKind(preset.kind);setLocal(preset.local);setApiKey('');setModelId('');setDefaultModel('');setDefaultEffort('');setReflection(false);setRpm(preset.requestsPerMinute?String(preset.requestsPerMinute):'');setAccountId('');setVision(false);setTestResult(null);setError('');setModal(true)}
 const goProvider=providers.value.find(p=>p.integration==='opencode-go'||p.baseUrl.includes('opencode.ai/zen/go')||p.baseUrl.includes('opencode.ai/inference/go'))
 const connectOpencodeGo=()=>{if(goProvider)open(goProvider);else openPreset(opencodeGoPreset)}
 const migratingHyperbolic=Boolean(edit&&edit.name==='Hyperbolic'&&updatedProviderEndpoint(edit)===baseUrl.trim())
 const needsNewKey=presetMode&&!freePreset&&(!local||integration==='freellmapi')||migratingHyperbolic
 const testConnection=async()=>{
  if(testBusy||!baseUrl.trim())return
  setTestBusy(true);setTestResult(null)
  try{
   const result=await post<{ok:boolean;message?:string;status?:number}>(`/api/providers/${edit?.id||'new'}/test`,{
    baseUrl:baseUrl.trim(),kind,integration,accountId:accountId.trim()||undefined,apiKey:apiKey.trim()||undefined,model:defaultModel.trim()||edit?.model,
   })
   setTestResult(result.ok?{ok:true,message:t('models.testOk')}:{ok:false,message:(result.message||t('models.testFail'))+(result.status?` (HTTP ${result.status})`:'')})
  }catch(e){setTestResult({ok:false,message:(e as Error).message})}
  finally{setTestBusy(false)}
 }
 const close=()=>{if(!saving)setModal(false)}
 const save=async()=>{
  if(saving||!baseUrl.trim()||(needsNewKey&&!apiKey.trim()))return
  if(reflection&&!modelId.trim()){setError(t('models.dlg.needReflectionModel'));return}
  setSaving(true);setError('')
  try{
   const saved=await post<Provider>('/api/providers',{
    ...edit,name:name.trim(),baseUrl:baseUrl.trim(),kind,...(presetMode?{auth:freePreset?'none':'api_key'}:{}),apiKey:apiKey.trim()||undefined,local,integration,accountId:accountId.trim()||undefined,requestsPerMinute:Number(rpm)||0,
    toolMode:edit?.kind===kind?edit.toolMode:undefined,
    capabilities:{...((edit?.capabilities as Record<string,unknown>|undefined)||{}),vision},
    reasoningEffort:defaultEffort,enabled:edit?.enabled??true,model:defaultModel.trim()||edit?.model,reflection,reflectionModel:reflection?modelId.trim():undefined,
   })
   providers.setValue(rows=>rows.some(p=>p.id===saved.id)?rows.map(p=>p.id===saved.id?saved:p):[...rows,saved])
   setCounts(rows=>{const next={...rows};delete next[saved.id];return next})
   setSavedId(saved.id);setRemovedName('');setApiKey('');setModal(false);setTab('providers');refresh('/api/providers')
   requestAnimationFrame(()=>{if(providerList.current)providerList.current.open=true})
  }catch(e){setError((e as Error).message)}finally{setSaving(false)}
 }
 const disconnect=async()=>{
  if(!edit||saving||edit.id==='dominion-native')return
  if(!window.confirm(t('models.dlg.disconnectConfirm',{name:edit.name})))return
  setSaving(true);setError('')
  try{
   await del('/api/providers/'+encodeURIComponent(edit.id))
   providers.setValue(rows=>rows.filter(provider=>provider.id!==edit.id))
   setCounts(rows=>{const next={...rows};delete next[edit.id];return next})
   setSavedId('');setRemovedName(edit.name);setApiKey('');setModal(false);refresh()
  }catch(e){setError((e as Error).message)}finally{setSaving(false)}
 }
 const startCodexDevice=async()=>{
  if(oauthBusy)return
  setOauthBusy('codex-device');setError('');setLoginMessage('');setLoginUrl('');setDeviceCode(null)
  try{
   const started=await post<{user_code:string;verification_url:string}>('/api/providers/codex/device',{})
   setDeviceCode({userCode:started.user_code,verificationUrl:started.verification_url})
   // The code and verification link are displayed below; the user opens it directly.
   setLoginPending(true)
  }catch(e){setOauthBusy('');setError((e as Error).message)}
 }
 const startCodexLogin=async()=>{
  if(oauthBusy)return
  // A blank tab must be opened synchronously from the click; after an
  // awaited API request browsers often block the OAuth login popup.
  const loginWindow=window.open('about:blank','_blank')
  setOauthBusy('codex-login');setError('');setLoginMessage('');setLoginUrl('');setDeviceCode(null)
  try{
   const started=await get<{url:string}>('/api/providers/codex/login')
   setLoginUrl(started.url)
   if(loginWindow)loginWindow.location.href=started.url
   setLoginPending(true)
   if(!loginWindow)setLoginMessage('팝업이 차단되었습니다. 아래 로그인 링크를 직접 누르세요.')
  }catch(e){loginWindow?.close();setOauthBusy('');setError((e as Error).message)}
 }
 useEffect(()=>{
  if(tab!=='oauth'&&tab!=='connect')return
  let alive=true
  get<{status:string;userCode?:string;verificationUrl?:string}>('/api/providers/codex/login/status').then(status=>{
   if(!alive||status.status!=='device_pending'||!status.userCode||!status.verificationUrl)return
   setDeviceCode({userCode:status.userCode,verificationUrl:status.verificationUrl})
   setLoginPending(true);setOauthBusy('codex-device')
  }).catch(()=>{})
  return()=>{alive=false}
 },[tab])
 useEffect(()=>{
  if(!loginPending)return
  let alive=true
  const timer=setInterval(async()=>{
   try{
    const status=await get<{status:string;message?:string}>('/api/providers/codex/login/status')
    if(!alive)return
    if(status.status==='success'){
     setLoginPending(false);setOauthBusy('');setDeviceCode(null);setLoginMessage(status.message||t('models.oauth.loginSuccess'))
     await Promise.all([providers.reload(),cli.reload()]);setSavedId('codex-oauth');refresh()
    }else if(status.status==='error'){
     setLoginPending(false);setOauthBusy('');setDeviceCode(null);setError(status.message||t('models.oauth.loginFailed'))
    }
   }catch{}
  },1500)
  return()=>{alive=false;clearInterval(timer)}
 },[loginPending])
 const startCliLogin=async(source:string)=>{
  if(oauthBusy)return
  // Open synchronously from the user click, as with Codex OAuth. Creating the
  // tab after the asynchronous Rust request would trigger popup blocking.
  const popup=window.open('about:blank','_blank')
  if(popup){
   popup.document.title='DOMINION · 로그인 연결 중'
   popup.document.body.textContent='공식 로그인 페이지를 준비하고 있습니다…'
  }
  cliLoginPopup.current=popup
  cliLoginPopupUrl.current=''
  setOauthBusy('login-'+source);setError('');setLoginMessage('')
  setCliAuth({source,status:'starting',message:'공식 로그인 창을 준비 중입니다.'})
  try{
   const started=await post<CliAuthStatus>('/api/providers/cli/'+encodeURIComponent(source)+'/login',{provider:source==='opencode'?openCodeAuthProvider:undefined})
   setCliAuth(started)
  }catch(e){popup?.close();cliLoginPopup.current=null;setCliAuth(null);setError((e as Error).message)}
  finally{setOauthBusy('')}
 }
 useEffect(()=>{
  if(!cliAuth)return
  if(cliAuth.url && cliLoginPopupUrl.current!==cliAuth.url){
   const popup=cliLoginPopup.current
   if(popup && !popup.closed){
    popup.location.href=cliAuth.url
    cliLoginPopupUrl.current=cliAuth.url
    cliLoginPopup.current=null
   }
  }
  if(['completed','failed','cancelled'].includes(cliAuth.status)){
   cliLoginPopup.current?.close()
   cliLoginPopup.current=null
  }
 },[cliAuth?.url,cliAuth?.status])
 useEffect(()=>{
  if(!cliAuth||['completed','failed','cancelled'].includes(cliAuth.status))return
  let live=true
  const source=cliAuth.source
  const timer=setInterval(()=>{
   void get<CliAuthStatus>('/api/providers/cli/'+encodeURIComponent(source)+'/login/status').then(next=>{
    if(!live)return
    setCliAuth(current=>current?.source===source?next:current)
    if(['completed','failed','cancelled'].includes(next.status)){
     void cli.reload().then(()=>providers.reload()).then(()=>{
      if(next.status==='completed')void fetchCliModels(source)
     })
     if(next.status==='completed')setLoginMessage('로그인 완료. 모델 목록을 불러옵니다.')
    }
   }).catch(e=>{if(live)setError((e as Error).message)})
  },1200)
  return()=>{live=false;clearInterval(timer)}
 },[cliAuth?.source,cliAuth?.status])
 const sendCliInput=async(input:string)=>{
  if(!cliAuth)return
  try{await post('/api/providers/cli/'+encodeURIComponent(cliAuth.source)+'/login/input',{input})}
  catch(e){setError((e as Error).message)}
 }
 const cancelCliAuth=async()=>{
  if(!cliAuth)return
  try{
   await post('/api/providers/cli/'+encodeURIComponent(cliAuth.source)+'/login/cancel',{})
   setCliAuth({...cliAuth,status:'cancelled',message:'로그인을 취소했습니다.'})
  }catch(e){setError((e as Error).message)}
 }
 const refreshFreeCatalog=async()=>{
  if(freeBusy)return
  setFreeBusy(true);setError('')
  try{const next=await post<FreeCatalog>('/api/providers/free-catalog/refresh',{});freeCatalog.setValue(next)}
  catch(e){setError((e as Error).message)}
  finally{setFreeBusy(false)}
 }
 const importCli=async(source:string)=>{
  if(oauthBusy)return
  setOauthBusy(source);setError('')
  try{
   const saved=await post<Provider&{importedCount?:number;skipped?:string[]}>('/api/providers/cli/import',{source})
   setSavedId(saved.id);await Promise.all([providers.reload(),cli.reload()]);refresh()
   if(source==='opencode-go')setLoginMessage('OpenCode Go 키를 DOMINION에 저장했습니다. 모델을 선택하고 연결을 검사하세요.')
   else if(saved.importedCount&&saved.importedCount>1)setLoginMessage(`API 키 ${saved.importedCount}개를 가져왔습니다.`)
  }catch(e){setError((e as Error).message)}
  finally{setOauthBusy('')}
 }
 const visiblePresets=useMemo(()=>providerPresets.filter(preset=>{
  if(tab==='local'&&preset.category!=='local')return false
  if(tab==='providers'&&preset.category==='local')return false
  if(tab==='catalog'||tab==='huggingface'||tab==='oauth')return false
  if(tab==='providers'&&category!=='all'&&preset.category!==category)return false
  const term=query.trim().toLowerCase()
  return !term||(preset.name+' '+preset.baseUrl).toLowerCase().includes(term)
 }),[tab,category,query])
 const visibleProviders=useMemo(()=>providers.value.filter(p=>{
  if(tab==='local'&&!p.local)return false
  if(tab==='providers'&&p.local)return false
  const term=query.trim().toLowerCase()
  return !term||(p.name+' '+p.baseUrl).toLowerCase().includes(term)
 }),[providers.value,query,tab])
 const catalogVisible=useMemo(()=>{const term=query.trim().toLowerCase();const unique=new Map<string,CatalogProvider>();for(const p of catalog.value.providers){const key=p.api.replace(/\/$/,'').toLowerCase();const old=unique.get(key);if(!old||p.models>old.models)unique.set(key,p)}return [...unique.values()].filter(p=>!term||(p.name+' '+p.id+' '+p.api).toLowerCase().includes(term)).sort((a,b)=>catalogSort==='models'?b.models-a.models||a.name.localeCompare(b.name):a.name.localeCompare(b.name))},[catalog.value.providers,query,catalogSort])
 const tabs:Array<{id:Tab;label:string;count?:number}>= [
  {id:'providers',label:t('models.simple.mine'),count:providers.value.filter(p=>!p.local).length},
  {id:'connect',label:t('models.simple.connect')},
  {id:'catalog',label:t('models.simple.catalog')},
  {id:'local',label:t('models.simple.local')},
 ]
 const moreTabs:Array<{id:Tab;label:string}>= [
  {id:'free',label:t('models.filter.free')},
  {id:'huggingface',label:t('models.simple.files')},
  {id:'oauth',label:t('models.simple.accounts')},
 ]
 const selectTab=(next:Tab)=>{setTab(next);setCategory('all');if(next==='connect')setBrowseProviderId('')}
 
 return {providers,cli,counts,setCounts,error,setError,saving,setSaving,savedId,setSavedId,removedName,setRemovedName,integration,setIntegration,freePreset,setFreePreset,providerList,modal,setModal,presetMode,setPresetMode,edit,setEdit,tab,setTab,category,setCategory,query,setQuery,catalogSort,setCatalogSort,catalogLimit,setCatalogLimit,browseProviderId,setBrowseProviderId,catalog,loginPending,setLoginPending,loginUrl,setLoginUrl,loginMessage,setLoginMessage,deviceCode,setDeviceCode,freeCatalog,freeBusy,setFreeBusy,oauthBusy,setOauthBusy,cliAuth,setCliAuth,cliLoginPopup,cliLoginPopupUrl,openCodeAuthProvider,setOpenCodeAuthProvider,cliModelCatalog,setCliModelCatalog,cliModelSelection,setCliModelSelection,cliModelBusy,setCliModelBusy,cliModelRequested,fetchCliModels,saveCliModel,name,setName,baseUrl,setBaseUrl,kind,setKind,apiKey,setApiKey,local,setLocal,modelId,setModelId,defaultModel,setDefaultModel,reflection,setReflection,rpm,setRpm,accountId,setAccountId,testBusy,setTestBusy,testResult,setTestResult,defaultEffort,setDefaultEffort,vision,setVision,editModels,probe,open,openPreset,goProvider,connectOpencodeGo,migratingHyperbolic,needsNewKey,testConnection,close,save,disconnect,startCodexDevice,startCodexLogin,startCliLogin,sendCliInput,cancelCliAuth,refreshFreeCatalog,importCli,visiblePresets,visibleProviders,catalogVisible,tabs,moreTabs,selectTab}
}
export type ModelHubController=ReturnType<typeof useModelHub>
