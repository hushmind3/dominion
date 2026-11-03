import type {LucideIcon} from 'lucide-react'

export function SidebarNavItem({icon:Icon,label,active,count,onClick}:{
 icon:LucideIcon;label:string;active:boolean;count?:number;onClick:()=>void
}){
 return <button type="button" onClick={onClick} aria-current={active?'page':undefined}
  className={['group relative flex h-10 w-full items-center gap-3 rounded-xl border-0 px-3 text-left text-[13px] font-semibold transition-colors',
   active?'bg-blue-50 text-blue-700':'bg-transparent text-slate-600 hover:bg-slate-50 hover:text-blue-700'].join(' ')}>
  <Icon size={18} strokeWidth={1.8} className="shrink-0"/>
  <span className="min-w-0 flex-1 truncate">{label}</span>
  {Boolean(count)&&<span className="rounded-full bg-blue-600 px-2 py-0.5 text-[10px] font-bold text-white">{count}</span>}
  {active&&<span className="absolute inset-y-2 right-0 w-1 rounded-full bg-blue-600"/>}
 </button>
}
