
import {ArrowRight, Check, KeyRound, LogIn, RefreshCw, ShieldCheck, X} from 'lucide-react'

import {Button, TextInput, SelectInput} from '../../components/Primitives'
import {FormField, Notice} from '../../components/UI'
import {DialogBackdrop, DialogPanel} from '../../components/Dialog'
import {ModelExplorer} from '../../components/ModelExplorer'
import {DominionCloudAccount} from '../../components/DominionCloudAccount'
import {ModelPicker} from '../../components/ModelPicker'
import {OAuthConnectionCard} from '../../components/OAuthConnectionCard'
import {CliAuthProgress} from '../../components/CliAuthProgress'
import {ReasoningSelect} from '../../components/ReasoningSelect'

import {t} from '../../i18n'

import type {ModelHubController} from './useModelHub'
import {providerPresets, updatedProviderEndpoint} from './catalog'

export function ConnectionTools({hub}:{hub:ModelHubController}){
 const {providers, cli, counts, error, removedName, modal, tab, setTab, query, setQuery, catalogSort, setCatalogSort, catalogLimit, setCatalogLimit, browseProviderId, setBrowseProviderId, catalog, loginPending, loginUrl, loginMessage, deviceCode, freeCatalog, freeBusy, oauthBusy, cliAuth, openCodeAuthProvider, setOpenCodeAuthProvider, cliModelCatalog, cliModelSelection, setCliModelSelection, cliModelBusy, fetchCliModels, saveCliModel, probe, openPreset, goProvider, connectOpencodeGo, startCodexDevice, startCodexLogin, startCliLogin, sendCliInput, cancelCliAuth, refreshFreeCatalog, importCli, catalogVisible}=hub
 return <div className="connection-tools">{tab==='free'&&<section className="mb-4 flex flex-wrap items-center gap-2 rounded-xl border border-divider bg-surface px-3 py-2 text-[11px] text-muted" title={t('models.freeCheck.note')}>
   <strong className="text-ink">{t('models.freeCheck.title')}</strong>
   <span className="rounded bg-success-soft px-1.5 py-0.5 font-semibold text-success">{t('models.freeCheck.free',{count:freeCatalog.value.counts?.free||0})}</span>
   <span className="rounded bg-canvas px-1.5 py-0.5 font-semibold text-muted">{t('models.freeCheck.unknown',{count:freeCatalog.value.counts?.unknown||0})}</span>
   <span className="rounded bg-canvas px-1.5 py-0.5 font-semibold text-brand">{t('models.freeCheck.paid',{count:freeCatalog.value.counts?.paid||0})}</span>
   {freeCatalog.value.providers.filter(p=>!p.reachable).length>0&&<span className="font-semibold text-danger">{t('models.freeCheck.failed',{count:freeCatalog.value.providers.filter(p=>!p.reachable).length})}</span>}
   <span className="ml-auto text-[10px] text-muted">{freeCatalog.value.updatedAt?t('models.freeCheck.updated',{time:new Date(freeCatalog.value.updatedAt).toLocaleString()}):t('models.freeCheck.empty')}</span>
   <Button type="button" variant="ghost" size="sm" disabled={freeBusy} onClick={()=>void refreshFreeCatalog()}>{freeBusy?<RefreshCw size={13} className="animate-spin"/>:<RefreshCw size={13}/>} {freeBusy?t('models.freeCheck.checking'):t('models.freeCheck.refresh')}</Button>
  </section>}
{tab==='free'&&<section className="mb-4 rounded-2xl border border-divider bg-surface p-4">
   <h2 className="mb-2 text-[14px] font-semibold text-ink">업체별 무료 모델 조회</h2>
   <p className="mb-3 text-[12px] text-muted">무료 여부는 제공 업체의 조회 결과를 기준으로 표시합니다. 실제 사용은 해당 업체 연결 상태에 따라 달라집니다.</p>
   {freeCatalog.value.providers.length===0?<p className="text-[12px] text-muted">조회된 모델이 없습니다. 위의 새로고침으로 확인하세요.</p>:<div className="grid gap-2 sm:grid-cols-2">{freeCatalog.value.providers.map(provider=><div key={provider.providerId} className="rounded-xl border border-divider bg-canvas p-3"><div className="flex items-center justify-between gap-2"><strong className="text-[12px] text-ink">{provider.providerName}</strong><span className={'text-[10px] '+(provider.reachable?'text-success':'text-danger')}>{provider.reachable?'조회됨':'연결 실패'}</span></div>{provider.error&&<p className="mt-1 text-[11px] text-danger">{provider.error}</p>}<div className="mt-2 max-h-40 space-y-1 overflow-auto">{provider.models.filter(model=>model.free===true).map(model=><p key={model.id} className="break-all text-[11px] text-muted">{model.id}</p>)}{!provider.models.some(model=>model.free===true)&&<p className="text-[11px] text-muted">무료로 분류된 모델 없음</p>}</div></div>)}</div>}
  </section>}
{tab==='catalog'&&browseProviderId&&<><Button variant="ghost" className="mb-4" onClick={()=>setBrowseProviderId('')}><ArrowRight size={14} className="rotate-180"/>{t('models.simple.catalog')}</Button><DominionCloudAccount key={browseProviderId} initialTab="providers" initialProviderId={browseProviderId} hideManagementNavigation onProviderAdded={providers.reload}/></>}
{tab==='catalog'&&!browseProviderId&&<section className="rounded-2xl border border-divider bg-surface p-5 shadow-sm">
   <div className="mb-3 flex flex-wrap items-start justify-between gap-2"><div><h2 className="m-0 text-[16px] font-bold text-ink">{t('models.catalog.title')}</h2><p className="mt-1 text-[12px] text-muted">{t('models.catalog.desc')}</p></div><Button type="button" variant="ghost" size="sm" disabled={catalog.loading} onClick={()=>void catalog.reload()}>{catalog.loading?<RefreshCw size={13} className="animate-spin"/>:<RefreshCw size={13}/>} {t('models.catalog.refresh')}</Button></div>
   <div className="mb-2 flex flex-wrap items-center gap-2"><TextInput value={query} onChange={e=>{setQuery(e.target.value);setCatalogLimit(60)}} placeholder={t('models.providerSearch')} className="min-w-[180px] flex-1 !text-[12px]"/><SelectInput aria-label="업체 정렬" value={catalogSort} onChange={e=>setCatalogSort(e.target.value as 'name'|'models')} className="!w-auto !text-[12px]"><option value="name">업체 이름순</option><option value="models">모델 많은 순</option></SelectInput><span className="text-[11px] text-muted">{catalogVisible.length}개 업체 · 중복 주소 제외</span></div>
   {catalog.error&&<Notice message={catalog.error}/>}
   {catalog.loading&&<p className="py-4 text-center text-[11px] text-muted">{t('models.catalog.loading')}</p>}
   <div className="grid max-h-[420px] grid-cols-1 gap-2 overflow-auto sm:grid-cols-2 xl:grid-cols-3">{catalogVisible.slice(0,catalogLimit).map(p=><div key={p.id} className="flex min-w-0 items-center justify-between gap-2 rounded-xl border border-divider bg-canvas px-3 py-2.5"><div className="min-w-0"><strong className="block truncate text-[12px] text-ink">{p.name}</strong><small className="block truncate text-[10px] text-muted">{p.api.replace(/^https?:\/\//,'')} · {t('models.catalog.modelCount',{count:p.models})}</small></div><Button type="button" size="sm" variant="secondary" onClick={()=>{setBrowseProviderId(p.id)}}>모델 보기</Button></div>)}</div>
   {catalogVisible.length>catalogLimit&&<div className="mt-3 text-center"><Button type="button" variant="secondary" onClick={()=>setCatalogLimit(n=>n+60)}>업체 더 보기 ({catalogVisible.length-catalogLimit}개 남음)</Button></div>}
  </section>}
{tab==='oauth'&&<section className="rounded-2xl border border-divider bg-surface p-5 shadow-sm">
   <div className="mb-4"><h2 className="m-0 text-[16px] font-bold text-ink">{t('models.oauth.title')}</h2><p className="mt-1 text-[12px] text-muted">{t('models.oauth.desc')}</p></div>
   <div className="grid grid-cols-1 items-stretch gap-3 md:grid-cols-2">
    {cli.value.map(item=><OAuthConnectionCard
     key={item.id}
     title={item.name}
     status={item.connected?'DOMINION 연결됨':item.available?'CLI 로그인 확인':'로그인 필요'}
     statusTone={item.connected?'connected':item.available?'available':'disconnected'}
     accountId={item.accountId}
     model={item.model}
     hint={item.hint}
    >
     {item.id==='codex'&&<>
      <Button type="button" variant="primary" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void startCodexLogin()}><LogIn size={13}/> {item.connected?'재로그인':'브라우저 로그인'}</Button>
      <Button type="button" variant="secondary" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void startCodexDevice()}><KeyRound size={13}/> 기기 코드 로그인</Button>
     </>}
     {item.id==='opencode'&&<SelectInput aria-label="OpenCode 로그인 제공자" className="!w-auto !min-w-[128px] !py-1 !text-[12px]" value={openCodeAuthProvider} onChange={event=>setOpenCodeAuthProvider(event.target.value)}>
      <option value="openai">OpenAI</option><option value="anthropic">Anthropic</option><option value="google">Google</option><option value="github-copilot">GitHub Copilot</option>
     </SelectInput>}
     {item.loginMethod==='browser-cli'&&<Button type="button" variant="primary" size="sm" disabled={Boolean(oauthBusy)||Boolean(cliAuth&&!['completed','failed','cancelled'].includes(cliAuth.status))} onClick={()=>void startCliLogin(item.id)}>{oauthBusy==='login-'+item.id?<RefreshCw size={13} className="animate-spin"/>:<LogIn size={13}/>} {item.available?'브라우저 재로그인':'브라우저 로그인'}</Button>}
     {item.id==='opencode-go'&&<Button type="button" variant="primary" size="sm" onClick={connectOpencodeGo}><KeyRound size={13}/> {goProvider?'키 / 모델 설정':'API 키 연결'}</Button>}
     {item.supported&&(item.importAvailable??item.available)&&<Button type="button" variant="secondary" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void importCli(item.id)}><KeyRound size={13}/> {item.id==='opencode-go'?'기존 키 가져오기':'기존 인증 가져오기'}</Button>}
     {(item.id==='claude'||item.id==='gemini')&&<Button type="button" variant="secondary" size="sm" onClick={()=>openPreset(providerPresets.find(p=>p.name===(item.id==='claude'?'Anthropic':'Google Gemini'))!)}><KeyRound size={13}/> 별도 API 연결(선택)</Button>}
     {item.connected&&['claude','gemini','opencode'].includes(item.id)&&<div className="flex w-full flex-wrap items-center gap-2 rounded-lg border border-divider bg-surface p-2">
      <SelectInput aria-label={item.name+' 사용 모델 선택'} className="min-w-[145px] flex-1 !py-1 !text-[12px]"
       value={cliModelSelection[item.id]||item.model||'default'}
       onChange={event=>setCliModelSelection(old=>({...old,[item.id]:event.target.value}))}>
       {(!cliModelCatalog[item.id]?.length)&&<option value={item.model||'default'}>{item.model||'CLI 기본 모델'}</option>}
       {(cliModelCatalog[item.id]||[]).map(model=><option key={model} value={model}>{model}</option>)}
      </SelectInput>
      <Button size="sm" type="button" variant="ghost" disabled={Boolean(cliModelBusy)} onClick={()=>void fetchCliModels(item.id)}><RefreshCw size={13}/> 모델 조회</Button>
      <Button size="sm" type="button" variant="secondary" disabled={Boolean(cliModelBusy)} onClick={()=>void saveCliModel(item.id)}>모델 적용</Button>
      <a className="rounded-lg bg-brand px-3 py-2 text-[12px] font-semibold text-white" href={'#/chat?provider=cli-'+item.id+'&model='+encodeURIComponent(cliModelSelection[item.id]||item.model||'default')}>채팅 시작</a>
     </div>}
     {item.id==='codex'&&item.connected&&<Button type="button" variant="ghost" size="sm" onClick={()=>{setTab('providers');const p=providers.value.find(p=>p.id==='codex-oauth');if(p)void probe(p)}}><RefreshCw size={13}/> 모델 확인</Button>}
     {item.id==='opencode-go'&&goProvider?.hasKey&&<Button type="button" variant="ghost" size="sm" disabled={counts[goProvider.id]?.busy} onClick={()=>void probe(goProvider)}><RefreshCw size={13}/> 모델 확인</Button>}
     <Button type="button" variant="ghost" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void cli.reload()}><RefreshCw size={13}/> 상태 새로고침</Button>
    </OAuthConnectionCard>)}
   </div>
   {cliAuth&&<CliAuthProgress session={cliAuth} onInput={sendCliInput} onCancel={cancelCliAuth}/>}
   {deviceCode&&<div className="mt-3 rounded-xl border border-divider bg-danger-soft px-4 py-3"><div className="text-[11px] text-muted">{t('models.oauth.deviceHint')}</div><div className="my-2 font-mono text-[26px] font-bold tracking-[.18em] text-brand-hover">{deviceCode.userCode}</div><a className="text-[11px] font-semibold text-brand hover:underline" href={deviceCode.verificationUrl} target="_blank" rel="noreferrer">{t('models.oauth.openDevice')}</a></div>}
   {loginPending&&!deviceCode&&<p className="mt-3 rounded-xl border border-divider bg-canvas px-3 py-2 text-[11px] text-muted">{t('models.oauth.loginHint')}{loginUrl&&<> · <a className="font-semibold text-brand hover:underline" href={loginUrl} target="_blank" rel="noreferrer">{t('models.oauth.openBrowser')}</a></>}</p>}
   {loginMessage&&<p className="mt-3 rounded-xl border border-success-soft bg-success-soft px-3 py-2 text-[11px] text-success">{loginMessage}</p>}
  </section>}
{tab==='local'&&<div className="mt-5"><ModelExplorer initialView="installed"/></div>}
{tab==='huggingface'&&<div className="mt-2"><ModelExplorer key="huggingface-browser" initialView="search" hideNavigation/></div>}
{removedName&&<p role="status" className="mb-4 mt-4 text-[12px] text-success">{t('models.dlg.disconnected',{name:removedName})}</p>}
{!modal&&<div className="mt-4"><Notice message={error||providers.error}/></div>}</div>
}

export function ConnectionDialog({hub}:{hub:ModelHubController}){
 const {error, saving, integration, freePreset, modal, presetMode, edit, name, setName, baseUrl, setBaseUrl, kind, setKind, apiKey, setApiKey, local, setLocal, modelId, setModelId, defaultModel, setDefaultModel, reflection, setReflection, rpm, setRpm, accountId, setAccountId, testBusy, testResult, defaultEffort, setDefaultEffort, vision, setVision, editModels, needsNewKey, testConnection, close, save, disconnect}=hub
 return <>{modal&&<DialogBackdrop onDismiss={close}><DialogPanel size="default">
   <div className="flex items-center justify-between border-b border-divider p-4"><h2 className="text-[15px] font-bold">{presetMode&&!edit?name+' '+t('models.editConnection'):edit?t('models.editConnection'):t('models.addConnection')}</h2><Button size="icon" variant="ghost" aria-label={t('models.close')} onClick={close} disabled={saving}><X size={16}/></Button></div>
   <div className="grid gap-3 p-4">
    {edit&&updatedProviderEndpoint(edit)&&<p role="status" className="rounded-lg border border-warning/20 bg-warning-soft px-3 py-2 text-[12px] text-warning">{t('models.endpointMigrated')}{edit.name==='Hyperbolic'?' '+t('models.endpointNewKey'):''}</p>}
    {presetMode&&!edit?<>
     <div className="rounded-xl border border-divider bg-canvas p-3">
      <strong className="block text-[13px] text-ink">{name}</strong>
      <p className="mt-1 text-xs text-muted">{t('connect.keyPrivacy')}</p>
     </div>
     {integration==='freellmapi'&&<FormField label={t('models.dlg.freeServer')}><TextInput value={baseUrl} onChange={e=>setBaseUrl(e.target.value)}/></FormField>}
     {baseUrl.includes('ACCOUNT_ID')&&<FormField label={t('models.dlg.accountId')} hint={t('models.dlg.accountIdHint')}><TextInput value={accountId} onChange={e=>setAccountId(e.target.value)} placeholder="a1b2c3d4e5f6..."/></FormField>}
     {(baseUrl.includes('ACCOUNT_ID')||baseUrl.includes('cloudflare.com'))&&<a className="text-[11px] font-semibold text-brand hover:underline" href="https://dash.cloudflare.com/profile/api-tokens" target="_blank" rel="noreferrer">{t('models.dlg.createCfToken')}</a>}
     {freePreset?<p className="text-[11px] leading-5 text-muted">{t('models.freeNote')}</p>:<FormField label={integration==='freellmapi'?t('models.dlg.integrationKey'):local?t('models.dlg.apiKeyOptional'):t('models.dlg.apiKey')} hint={integration==='freellmapi'?t('models.dlg.integrationHint'):undefined}><TextInput autoFocus type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder={t('models.dlg.pasteKey')}/></FormField>}
    </>:<>
     <FormField label={t('models.dlg.connectionName')} hint={t('models.dlg.connectionNameHint')}><TextInput value={name} onChange={e=>setName(e.target.value)} placeholder={t('models.dlg.connectionNamePlaceholder')}/></FormField>
     <FormField label={t('models.dlg.serverUrl')}><TextInput value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder="http://127.0.0.1:1234/v1"/></FormField>
     {baseUrl.includes('ACCOUNT_ID')&&<FormField label={t('models.dlg.accountId')} hint={t('models.dlg.accountIdHint')}><TextInput value={accountId} onChange={e=>setAccountId(e.target.value)} placeholder="a1b2c3d4e5f6..."/></FormField>}
     <FormField label={t('models.dlg.apiMode')}><SelectInput value={kind} onChange={e=>setKind(e.target.value)}><option value="openai">{t('models.dlg.openaiCompat')}</option><option value="anthropic">Anthropic</option><option value="codex">Codex (OAuth)</option><option value="ollama">Ollama</option></SelectInput></FormField>
     {kind==='codex'&&edit&&<FormField label="Codex 기본 모델" hint="계정 모델 목록에서 선택합니다. 모델이 보이지 않으면 OAuth 연결을 갱신하세요."><ModelPicker providerId={edit.id} catalogue={editModels} value={defaultModel} onChange={model=>{setDefaultModel(model);setDefaultEffort('')}} className="w-full"/></FormField>}
     <FormField label={t('chat.reasoningEffort')} hint="에이전트나 대화에서 별도로 지정하지 않았을 때 사용하는 기본값입니다."><ReasoningSelect value={defaultEffort} onChange={setDefaultEffort} model={editModels.value.find(item=>item.id===(defaultModel||edit?.model))} requireCatalog={kind==='codex'}/></FormField>
     {kind!=='codex'&&<FormField label={t('models.dlg.apiKey')}><TextInput type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder={edit?.hasKey?t('models.dlg.keepKey'):t('models.dlg.keyIfNeeded')}/></FormField>}
     <label className="flex items-center gap-2 text-[12px]"><TextInput type="checkbox" checked={local} onChange={e=>setLocal(e.target.checked)}/> {t('models.dlg.localServer')}</label>
     <label className="flex items-center gap-2 text-[12px]"><TextInput type="checkbox" checked={vision} onChange={e=>setVision(e.target.checked)}/> {t('models.dlg.vision')}</label>
     <p className="-mt-2 text-[11px] leading-5 text-muted">{t('models.dlg.visionHint')}</p>
     {edit&&<details open={reflection||undefined} className="rounded-xl border border-divider bg-surface p-3">
      <summary className="cursor-pointer text-[12px]">{t('models.dlg.reflection')}</summary>
      <label className="my-2 flex items-center gap-2 text-[12px]"><TextInput type="checkbox" checked={reflection} onChange={e=>setReflection(e.target.checked)}/> {t('models.dlg.useReflection')}</label>
      {reflection&&<FormField label={t('models.dlg.reflectionModel')} hint={t('models.dlg.reflectionHint')}>
       {baseUrl.trim()===edit.baseUrl&&kind===edit.kind&&!apiKey.trim()
        ?<ModelPicker providerId={edit.id} value={modelId} onChange={setModelId} className="w-full"/>
        :<TextInput value={modelId} onChange={e=>setModelId(e.target.value)} placeholder={t('models.dlg.modelIdPlaceholder')}/>}
      </FormField>}
     </details>}
    </>}
    <details className="rounded-xl border border-divider p-3"><summary className="cursor-pointer text-xs text-muted">{t('connect.advanced')}</summary><div className="mt-3 grid gap-3">
    {kind!=='codex'&&<FormField label="기본 모델 ID" hint="키를 등록한 후 제공자 모델 목록에서 선택하거나 ID를 직접 입력하세요.">
     <div className="grid gap-2">
      {edit?.hasKey&&baseUrl.trim()===edit.baseUrl&&kind===edit.kind&&<ModelPicker providerId={edit.id} value={defaultModel} onChange={setDefaultModel} className="w-full"/>}
      <TextInput value={defaultModel} onChange={e=>setDefaultModel(e.target.value)} placeholder={integration==='opencode-go'?'예: deepseek-v4.1-flash':t('models.dlg.modelIdPlaceholder')}/>
     </div>
    </FormField>}
    <FormField label={t('models.dlg.rpm')} hint={t('models.dlg.rpmHint')}><TextInput type="number" min={0} max={10000} value={rpm} onChange={e=>setRpm(e.target.value)} placeholder="0"/></FormField></div></details>
    {error&&<Notice message={error}/>}
    {testResult&&<div className={'rounded-xl border px-3 py-2 text-[12px] '+(testResult.ok?'border-success-soft bg-success-soft text-success':'border-divider bg-danger-soft text-brand')}>{testResult.message}</div>}
   </div>
   <div className="flex justify-end gap-2 border-t border-divider p-4">{edit&&edit.id!=='dominion-native'&&<Button variant="danger" className="mr-auto" disabled={saving} onClick={()=>void disconnect()}>{t('models.disconnect')}</Button>}<Button variant="ghost" disabled={saving||testBusy||!baseUrl.trim()} onClick={()=>void testConnection()}>{testBusy?<RefreshCw size={14} className="animate-spin"/>:<ShieldCheck size={14}/>} {testBusy?t('models.testing'):t('models.test')}</Button><Button onClick={close} disabled={saving}>{t('models.cancel')}</Button><Button variant="primary" disabled={saving||!baseUrl.trim()||(needsNewKey&&!apiKey.trim())} onClick={()=>void save()}><Check size={14}/> {saving?t('models.saving'):t('models.save')}</Button></div>
  </DialogPanel></DialogBackdrop>}</>
}
