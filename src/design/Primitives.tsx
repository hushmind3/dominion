import {useEffect,useId,useState,type ButtonHTMLAttributes,type InputHTMLAttributes,type SelectHTMLAttributes,type TextareaHTMLAttributes,type ReactNode,type FormEvent} from 'react'
import {AlertCircle,ArrowRight,Check,ChevronDown,LoaderCircle,Search,X} from 'lucide-react'

type Child={children:ReactNode}
const gaps={none:'gap-0',xs:'gap-1.5',sm:'gap-2.5',md:'gap-4',lg:'gap-6',xl:'gap-8'} as const
type Gap=keyof typeof gaps
const muted='text-muted'
export const cx=(...items:(string|false|null|undefined)[])=>items.filter(Boolean).join(' ')

export function Page({children,wide=false}:{children:ReactNode;wide?:boolean}){
 return <div className={cx('mx-auto w-full px-4 pb-16 pt-6 sm:px-6 sm:pt-8 lg:px-9',wide?'max-w-[1700px]':'max-w-[1450px]')}>{children}</div>
}
export function PageHeader({eyebrow,title,description,actions}:{eyebrow?:string;title:string;description?:string;actions?:ReactNode}){
 return <div className="mb-7 flex flex-wrap items-start justify-between gap-4">
  <div className="min-w-0"><p className="mb-2 text-[11px] font-extrabold tracking-[.12em] text-brand">{eyebrow||'DOMINION'}</p><h1 className="text-[clamp(24px,2.4vw,34px)] font-extrabold tracking-[-.04em] text-ink">{title}</h1>{description&&<p className="mt-2 max-w-3xl text-[13px] leading-relaxed text-muted">{description}</p>}</div>
  {actions&&<div className="flex flex-wrap items-center gap-2">{actions}</div>}
 </div>
}
export function Stack({children,gap='md',className}:{children:ReactNode;gap?:Gap;className?:string}){
 return <div className={cx('flex min-w-0 flex-col',gaps[gap],className)}>{children}</div>
}
export function Row({children,gap='md',align='center',justify='start',wrap=false,className}:{children:ReactNode;gap?:Gap;align?:'center'|'start'|'end'|'stretch';justify?:'start'|'between'|'end';wrap?:boolean;className?:string}){
 const alignClass={center:'items-center',start:'items-start',end:'items-end',stretch:'items-stretch'}[align]
 const justifyClass={start:'justify-start',between:'justify-between',end:'justify-end'}[justify]
 return <div className={cx('flex min-w-0',gaps[gap],alignClass,justifyClass,wrap&&'flex-wrap',className)}>{children}</div>
}
export function Grid({children,cols=2,gap='md'}:{children:ReactNode;cols?:1|2|3|4;gap?:Gap}){
 const columns={1:'grid-cols-1',2:'grid-cols-1 md:grid-cols-2',3:'grid-cols-1 md:grid-cols-2 xl:grid-cols-3',4:'grid-cols-1 sm:grid-cols-2 xl:grid-cols-4'}[cols]
 return <div className={cx('grid min-w-0',columns,gaps[gap])}>{children}</div>
}
export function Card({children,pad='md',tone='default'}:{children:ReactNode;pad?:'none'|'sm'|'md'|'lg';tone?:'default'|'soft'|'subtle'}){
 return <section className={cx('min-w-0 overflow-hidden rounded-2xl border border-line shadow-panel',tone==='default'?'bg-surface':tone==='soft'?'bg-brand-soft':'bg-canvas', {none:'',sm:'p-4',md:'p-5 sm:p-6',lg:'p-6 sm:p-8'}[pad])}>{children}</section>
}
export function Section({title,subtitle,actions,children}:{title:string;subtitle?:string;actions?:ReactNode;children:ReactNode}){
 return <Stack gap="sm"><div className="flex flex-wrap items-center justify-between gap-3"><div><h2 className="text-[16px] font-bold tracking-tight text-ink">{title}</h2>{subtitle&&<p className="mt-1 text-xs text-subtle">{subtitle}</p>}</div>{actions}</div>{children}</Stack>
}
export function Divider(){return <div role="separator" className="my-1 h-px bg-line"/>}
export function Text({children,tone='default',size='md',weight='normal'}:{children:ReactNode;tone?:'default'|'muted'|'subtle'|'brand'|'positive'|'negative';size?:'xs'|'sm'|'md'|'lg';weight?:'normal'|'semibold'|'bold'}){
 return <span className={cx({default:'text-ink',muted:'text-muted',subtle:'text-subtle',brand:'text-brand',positive:'text-positive',negative:'text-negative'}[tone],{xs:'text-[11px]',sm:'text-xs',md:'text-[13px]',lg:'text-base'}[size],{normal:'font-normal',semibold:'font-semibold',bold:'font-bold'}[weight])}>{children}</span>
}
export function Badge({children,tone='neutral'}:{children:ReactNode;tone?:'neutral'|'brand'|'positive'|'warning'|'negative'}){
 const color={neutral:'bg-canvas text-muted',brand:'bg-brand-soft text-brand',positive:'bg-positive-soft text-positive',warning:'bg-warning-soft text-warning',negative:'bg-negative-soft text-negative'}[tone]
 return <span className={cx('inline-flex max-w-full items-center gap-1 rounded-md px-2.5 py-1 text-[10px] font-bold',color)}>{children}</span>
}
type ButtonVariant='primary'|'secondary'|'ghost'|'danger'|'quiet'
export function Button({children,variant='secondary',size='md',loading=false,className,...props}:ButtonHTMLAttributes<HTMLButtonElement>&{variant?:ButtonVariant;size?:'sm'|'md'|'lg';loading?:boolean}){
 const variantClass={primary:'bg-brand text-white hover:bg-brand-hover shadow-sm',secondary:'border border-line bg-white text-ink hover:bg-canvas',ghost:'text-muted hover:bg-canvas hover:text-ink',danger:'bg-negative-soft text-negative hover:bg-red-100',quiet:'bg-brand-soft text-brand hover:bg-blue-100'}[variant]
 const dimensions={sm:'min-h-8 px-3 py-1.5 text-[11px]',md:'min-h-10 px-4 py-2 text-xs',lg:'min-h-11 px-5 py-2.5 text-[13px]'}[size]
 return <button type="button" {...props} disabled={loading||props.disabled} className={cx('inline-flex flex-shrink-0 items-center justify-center gap-2 rounded-[10px] font-semibold transition-colors focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50',variantClass,dimensions,className)}>{loading&&<LoaderCircle size={15} className="animate-spin"/>}{children}</button>
}
export function IconButton({icon,label,variant='ghost',size='md',...props}:Omit<ButtonHTMLAttributes<HTMLButtonElement>,'children'|'aria-label'>&{icon:ReactNode;label:string;variant?:ButtonVariant;size?:'sm'|'md'}){
 return <Button {...props} aria-label={label} title={props.title||label} size={size} variant={variant} className={size==='sm'?'!w-8 !px-0':'!w-10 !px-0'}>{icon}</Button>
}
export function Input({label,help,error,...props}:InputHTMLAttributes<HTMLInputElement>&{label?:string;help?:string;error?:string}){
 const id=useId()
 return <label className="flex min-w-0 flex-1 flex-col gap-1.5"><span className={cx('text-xs font-semibold text-ink',!label&&'sr-only')}>{label||props.placeholder||'입력'}</span><input id={id} {...props} className={cx('min-h-10 w-full min-w-0 rounded-[10px] border bg-white px-3.5 py-2.5 text-[13px] text-ink placeholder:text-subtle outline-none focus:border-brand disabled:bg-canvas',error?'border-negative':'border-line',props.className)}/>{help&&<span className="text-[11px] text-subtle">{help}</span>}{error&&<span className="text-[11px] text-negative">{error}</span>}</label>
}
export function SearchBox({value,onChange,placeholder='검색',label='검색'}:{value:string;onChange:(value:string)=>void;placeholder?:string;label?:string}){
 return <div className="flex min-w-0 flex-1 items-center gap-2 rounded-[10px] border border-line bg-white px-3"><Search size={17} className="shrink-0 text-subtle"/><input aria-label={label} value={value} onChange={e=>onChange(e.target.value)} placeholder={placeholder} className="h-11 w-full min-w-0 bg-transparent text-[13px] text-ink placeholder:text-subtle outline-none"/>{value&&<IconButton size="sm" icon={<X size={15}/>} label="검색어 지우기" onClick={()=>onChange('')}/>}</div>
}
export function Textarea({label,rows=4,...props}:TextareaHTMLAttributes<HTMLTextAreaElement>&{label?:string}){
 return <label className="flex min-w-0 flex-1 flex-col gap-1.5"><span className={cx('text-xs font-semibold text-ink',!label&&'sr-only')}>{label||props.placeholder||'내용'}</span><textarea {...props} rows={rows} className={cx('w-full min-w-0 resize-y rounded-[10px] border border-line bg-white px-3.5 py-3 text-[13px] leading-relaxed text-ink placeholder:text-subtle focus:border-brand outline-none',props.className)}/></label>
}
export function Select({label,items,placeholder,...props}:Omit<SelectHTMLAttributes<HTMLSelectElement>,'children'>&{label?:string;items:{value:string;label:string}[];placeholder?:string}){
 return <label className="flex min-w-0 flex-col gap-1.5"><span className={cx('text-xs font-semibold text-ink',!label&&'sr-only')}>{label||'선택'}</span><span className="relative"><select {...props} className={cx('min-h-10 w-full appearance-none rounded-[10px] border border-line bg-white py-2.5 pl-3 pr-9 text-[13px] text-ink outline-none focus:border-brand',props.className)}>{placeholder&&<option value="">{placeholder}</option>}{items.map(x=><option key={x.value} value={x.value}>{x.label}</option>)}</select><ChevronDown size={15} className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-subtle"/></span></label>
}
export function Toggle({checked,onChange,label}:{checked:boolean;onChange:(v:boolean)=>void;label:string}){
 return <label className="inline-flex cursor-pointer items-center gap-2 text-xs text-muted"><input type="checkbox" checked={checked} onChange={e=>onChange(e.target.checked)} className="size-4 accent-brand"/>{label}</label>
}
export function Tabs({value,onChange,items}:{value:string;onChange:(v:string)=>void;items:{value:string;label:string;count?:number}[]}){
 return <div role="tablist" className="flex max-w-full items-center gap-1 overflow-x-auto border-b border-line">
  {items.map(item=><Button key={item.value} role="tab" aria-selected={value===item.value} size="md" variant="ghost" onClick={()=>onChange(item.value)} className={cx('!rounded-none !border-b-2 !px-4 !font-bold !whitespace-nowrap',value===item.value?'!border-brand !text-brand':'!border-transparent !text-subtle')}>{item.label}{item.count!==undefined&&<Badge tone={value===item.value?'brand':'neutral'}>{item.count}</Badge>}</Button>)}
 </div>
}
export function Empty({icon,title,description,action}:{icon?:ReactNode;title:string;description?:string;action?:ReactNode}){
 return <div className="flex min-h-44 flex-col items-center justify-center gap-3 p-8 text-center"><div className="grid size-12 place-items-center rounded-2xl bg-canvas text-subtle">{icon||<Search size={25}/>}</div><h3 className="text-sm font-bold text-ink">{title}</h3>{description&&<p className="max-w-sm text-xs leading-relaxed text-subtle">{description}</p>}{action}</div>
}
export function Alert({message,tone='negative',onClose}:{message:string;tone?:'negative'|'warning'|'positive';onClose?:()=>void}){
 if(!message)return null
 const appearance={negative:'border-red-100 bg-negative-soft text-negative',warning:'border-amber-100 bg-warning-soft text-warning',positive:'border-green-100 bg-positive-soft text-positive'}[tone]
 return <div role="alert" className={cx('flex items-start gap-2 rounded-xl border p-3 text-xs leading-relaxed',appearance)}><AlertCircle size={16} className="shrink-0"/><span className="flex-1 break-words">{message}</span>{onClose&&<IconButton icon={<X size={14}/>} label="알림 닫기" size="sm" onClick={onClose}/>}</div>
}
export function Dialog({open,onClose,title,description,children,footer,wide=false}:{open:boolean;onClose:()=>void;title:string;description?:string;children:ReactNode;footer?:ReactNode;wide?:boolean}){
 useEffect(()=>{if(!open)return;const handle=(e:KeyboardEvent)=>{if(e.key==='Escape')onClose()};window.addEventListener('keydown',handle);return()=>window.removeEventListener('keydown',handle)},[open,onClose])
 if(!open)return null
 return <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/50 p-3 backdrop-blur-sm" onMouseDown={onClose} role="presentation">
  <section role="dialog" aria-modal="true" aria-label={title} onMouseDown={e=>e.stopPropagation()} className={cx('flex max-h-[92vh] w-full flex-col overflow-hidden rounded-[20px] bg-white shadow-dialog',wide?'max-w-3xl':'max-w-xl')}>
   <header className="flex items-start justify-between gap-4 border-b border-line px-5 py-5 sm:px-6"><div><h2 className="text-lg font-bold text-ink">{title}</h2>{description&&<p className="mt-1 text-xs text-subtle">{description}</p>}</div><IconButton icon={<X size={19}/>} label="닫기" onClick={onClose}/></header>
   <div className="min-h-0 overflow-y-auto p-5 sm:p-6">{children}</div>
   {footer&&<footer className="flex flex-wrap justify-end gap-2 border-t border-line px-5 py-4 sm:px-6">{footer}</footer>}
  </section>
 </div>
}
export function Drawer({open,onClose,title,children,footer}:{open:boolean;onClose:()=>void;title:string;children:ReactNode;footer?:ReactNode}){
 if(!open)return null
 return <div className="fixed inset-0 z-[80] flex justify-end bg-black/45" onMouseDown={onClose} role="presentation"><div className="flex h-full w-full max-w-[540px] flex-col bg-white shadow-dialog" role="dialog" aria-modal="true" aria-label={title} onMouseDown={e=>e.stopPropagation()}><div className="flex items-center justify-between border-b border-line p-5"><h2 className="text-lg font-bold">{title}</h2><IconButton label="닫기" icon={<X size={18}/>} onClick={onClose}/></div><div className="min-h-0 flex-1 overflow-y-auto p-5">{children}</div>{footer&&<div className="border-t border-line p-4">{footer}</div>}</div></div>
}
export function Kpi({label,value,caption,icon,onClick}:{label:string;value:ReactNode;caption?:string;icon?:ReactNode;onClick?:()=>void}){
 return <Card><div className="flex min-h-28 flex-col justify-between gap-3"><div className="flex items-start justify-between gap-2"><span className="text-xs font-semibold text-muted">{label}</span>{icon&&<span className="grid size-9 place-items-center rounded-[11px] bg-brand-soft text-brand">{icon}</span>}</div><div className="flex items-end justify-between gap-2"><div><p className="text-[26px] font-extrabold tracking-tight text-ink">{value}</p>{caption&&<p className="mt-1 text-[11px] text-subtle">{caption}</p>}</div>{onClick&&<IconButton icon={<ArrowRight size={17}/>} label={label+' 열기'} onClick={onClick}/>}</div></div></Card>
}
export function ListItem({title,subtitle,meta,leading,actions,onClick}:{title:ReactNode;subtitle?:ReactNode;meta?:ReactNode;leading?:ReactNode;actions?:ReactNode;onClick?:()=>void}){
 return <div className="flex min-w-0 flex-wrap items-center gap-3 border-b border-line px-4 py-3.5 last:border-b-0 hover:bg-canvas/60">
  {leading&&<span className="grid size-10 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand">{leading}</span>}
  <div className="min-w-0 flex-1">{onClick?<Button variant="ghost" size="sm" onClick={onClick} className="!h-auto !justify-start !p-0 !text-left !text-ink !text-[13px]">{title}</Button>:<p className="truncate text-[13px] font-bold text-ink">{title}</p>}{subtitle&&<div className="mt-1 truncate text-[11px] text-subtle">{subtitle}</div>}</div>
  {meta&&<div className="text-[11px] text-muted">{meta}</div>}{actions&&<div className="flex flex-wrap items-center gap-1.5">{actions}</div>}
 </div>
}
export function Progress({value,total=100,label,detail}:{value:number;total?:number;label?:string;detail?:string}){
 const v=Math.max(0,Math.min(total,value))
 return <div className="flex flex-col gap-1.5"><div className="flex items-center justify-between gap-3"><Text size="xs" weight="bold">{label||'진행률'}</Text><Text tone="subtle" size="xs">{detail||Math.round(100*v/Math.max(total,1))+'%'}</Text></div><progress value={v} max={Math.max(1,total)} className="h-2 w-full overflow-hidden rounded-full accent-brand"/></div>
}
export function DataTable<T>({rows,columns,keyOf,empty='표시할 항목이 없습니다.'}:{rows:T[];columns:{key:string;header:string;render:(item:T)=>ReactNode}[];keyOf:(r:T)=>string;empty?:string}){
 if(!rows.length)return <Empty title={empty}/>
 return <div className="w-full overflow-x-auto"><table className="w-full border-collapse text-left"><thead className="border-b border-line bg-canvas"><tr>{columns.map(c=><th key={c.key} className="whitespace-nowrap px-4 py-3 text-[11px] font-bold text-muted">{c.header}</th>)}</tr></thead><tbody>{rows.map(r=><tr key={keyOf(r)} className="border-b border-line last:border-0 hover:bg-canvas/60">{columns.map(c=><td key={c.key} className="px-4 py-3.5 text-xs text-ink">{c.render(r)}</td>)}</tr>)}</tbody></table></div>
}
export function Spinner({label='불러오는 중'}:{label?:string}){return <div role="status" className="flex items-center justify-center gap-2 p-6 text-xs text-subtle"><LoaderCircle size={18} className="animate-spin"/>{label}</div>}
export function Confirmation({open,onClose,onConfirm,title,message,confirmLabel='삭제'}:{open:boolean;onClose:()=>void;onConfirm:()=>void;title:string;message:string;confirmLabel?:string}){
 return <Dialog open={open} onClose={onClose} title={title} footer={<><Button onClick={onClose}>취소</Button><Button variant="danger" onClick={()=>{onConfirm();onClose()}}>{confirmLabel}</Button></>}><p className="text-[13px] leading-relaxed text-muted">{message}</p></Dialog>
}
