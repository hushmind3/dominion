import {Notice} from './UI'
import {useEffect,useState} from 'react'
import {createPortal} from 'react-dom'
import {Check,ChevronDown,ShieldAlert,ShieldCheck} from 'lucide-react'
import {get,post} from '../api'
import {Button} from './Primitives'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {matchesRefreshTarget,type RefreshScope} from '../refreshEvents'
import {t,useLocale,type LocaleKey} from '../i18n'

type PermissionMode='request'|'workspace'|'full'
const permissionCopy:Record<PermissionMode,{label:LocaleKey;detail:LocaleKey}>={
 request:{label:'chat.perm.request.label',detail:'chat.perm.request.detail'},
 workspace:{label:'chat.perm.workspace.label',detail:'chat.perm.workspace.detail'},
 full:{label:'chat.perm.full.label',detail:'chat.perm.full.detail'}
}
/** Shared by direct and group chat; changing modes never changes policy by itself. */
export function ExecutionPermissions({open,onOpenChange,onError,onChanged,active=true}:{open:boolean;onOpenChange:(open:boolean)=>void;onError:(error:string)=>void;onChanged?:(label:string)=>void;active?:boolean}){
 const locale=useLocale()
 const [permissionMode,setPermissionMode]=useState<PermissionMode>('request'),[permissionBusy,setPermissionBusy]=useState(false)
 const permissionPanel=useAnchoredPanel(open,()=>onOpenChange(false),340,'top')
 useEffect(()=>{
  if(!active)return
  const controller=new AbortController()
  let latest=0
  const load=()=>{
   const request=++latest
   get<{autoApprove?:string[];autoApproveAll?:boolean}>('/api/policy',controller.signal).then(p=>{
    if(!controller.signal.aborted&&request===latest)
     setPermissionMode(p.autoApproveAll?'full':(p.autoApprove||[]).includes('workspace_write')?'workspace':'request')
   }).catch(()=>{})
  }
  const onPolicyChanged=(event:Event)=>{
   const targets=(event as CustomEvent<RefreshScope>).detail?.targets||[]
   if(matchesRefreshTarget('/api/policy',targets))load()
  }
  load()
  window.addEventListener('dominion-data-change',onPolicyChanged)
  return()=>{controller.abort();window.removeEventListener('dominion-data-change',onPolicyChanged)}
 },[active,open])
 const changePermission=async(mode:PermissionMode)=>{
  if(permissionBusy)return
  if(mode==='full'&&!window.confirm(t('chat.perm.fullConfirm')))return
  setPermissionBusy(true);onError('')
  try{
   await post('/api/policy',mode==='full'?{id:'default',autoApprove:[],autoApproveAll:true,autoApproveMcp:true}
    :mode==='workspace'?{id:'default',autoApprove:['workspace_write'],autoApproveAll:false,autoApproveMcp:false}
    :{id:'default',autoApprove:[],autoApproveAll:false,autoApproveMcp:false})
   setPermissionMode(mode);onOpenChange(false)
   onChanged?.(t(permissionCopy[mode].label))
  }catch(e){onError((e as Error).message)}finally{setPermissionBusy(false)}
 }
 const activePermission=permissionCopy[permissionMode]
 return <div className="relative">
  <Button type="button" variant="ghost" ref={permissionPanel.trigger} onClick={()=>onOpenChange(!open)}
   className={'chat-permission inline-flex !min-h-9 items-center gap-2 !rounded-lg !border-0 !px-3 !text-[12px] font-semibold '+(permissionMode==='full'?'!bg-amber-50 text-amber-700 hover:!bg-amber-100':'!bg-[var(--color-canvas)] text-[var(--color-muted)] hover:!bg-[var(--color-canvas)]')}
   title={t(activePermission.detail)} aria-label={t('chat.permissions')+': '+t(activePermission.label)} aria-expanded={open} aria-haspopup="dialog">
   {permissionMode==='full'?<ShieldAlert size={16}/>:<ShieldCheck size={16}/>}
   <span className="truncate">{t(activePermission.label)}</span><ChevronDown size={14}/>
  </Button>
  {open&&permissionPanel.position&&createPortal(
   <div ref={permissionPanel.panel} style={permissionPanel.position} role="dialog" aria-label={t('chat.permissions')}
    className="z-[80] max-w-full overflow-y-auto overscroll-contain rounded-2xl border border-divider bg-surface p-3 shadow-xl">
    <div className="border-b border-[var(--color-canvas)] px-2 pb-3">
     <h3 className="text-[15px] font-bold text-[var(--color-ink)]">{t('chat.permissions')}</h3>
     <p className="mt-1 text-[12px] leading-5 text-[var(--color-muted)]">{t('chat.permissionsDesc')}</p>
    </div>
    <div className="space-y-1 pt-2">
     {(['request','workspace','full'] as PermissionMode[]).map(mode=>
      <Button type="button" variant="ghost" key={mode} disabled={permissionBusy}
       aria-pressed={permissionMode===mode} onClick={()=>void changePermission(mode)}
       className={'flex w-full !min-h-16 items-start gap-3 !justify-start rounded-xl !border-0 px-3 py-3 text-left hover:bg-[var(--color-canvas)] focus-visible:outline-2 focus-visible:outline-offset-2 '+(permissionMode===mode?'!bg-[#F8F1E8]':'')}>
       <span className={'mt-1 grid size-5 shrink-0 place-items-center rounded-full border '+(permissionMode===mode?'border-brand bg-brand text-white':'border-[var(--color-divider)]')}>
        {permissionMode===mode&&<Check size={13}/>}
       </span>
       <span className="min-w-0 flex-1">
        <strong className={'block text-[13px] '+(mode==='full'?'text-amber-700':'text-[var(--color-ink)]')}>
         {t(permissionCopy[mode].label)}
        </strong>
        <small className="mt-1 block text-[12px] leading-5 text-[var(--color-muted)]">
         {t(permissionCopy[mode].detail)}
        </small>
       </span>
       {permissionMode===mode&&<span className="shrink-0 text-[11px] font-bold text-[var(--color-brand)]">
        {locale==='ko'?'현재 설정':'Active'}
       </span>}
      </Button>
     )}
    </div>
   </div>,permissionPanel.trigger.current?.closest('[role="dialog"]')||document.body)}
 </div>
}

/** Settings owns the popover and reports errors beside the control. */
export function ExecutionPermissionsSetting() {
 const [open, setOpen] = useState(false)
 const [error, setError] = useState('')
 return <div>
  <ExecutionPermissions open={open} onOpenChange={setOpen} onError={setError}/>
  <Notice message={error}/>
 </div>
}
