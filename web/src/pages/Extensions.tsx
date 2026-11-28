import {useState} from 'react'
import {Button} from '../components/Primitives'
import {PluginCatalog} from './extensions/PluginCatalog'
import {SkillLibrary} from './extensions/SkillLibrary'
import {useLocale} from '../i18n'
export type ExtensionTab='discover'|'market'|'installed'|'plugins'|'skills'|'apps'
/** One discovery surface. Transport and installation state are filters, not separate pages. */
export function ExtensionsPage({initialCategory='all'}:{initialCategory?:string}={}){
 const en=useLocale()==='en'
 const [tab,setTab]=useState<'plugins'|'skills'>('plugins')
 return <div className="dominion-plugin-workspace flex min-h-0 flex-1 flex-col">
  <nav className="flex shrink-0 gap-7 border-b border-divider px-8" aria-label={en?'Customize':'사용자 설정'}>
   <Button variant="bare" className={'border-b-2 px-1 py-4 text-sm font-semibold '+(tab==='plugins'?'border-brand text-brand':'border-transparent text-muted')} aria-current={tab==='plugins'?'page':undefined} onClick={()=>setTab('plugins')}>{en?'Plugins':'플러그인'}</Button>
   <Button variant="bare" className={'border-b-2 px-1 py-4 text-sm font-semibold '+(tab==='skills'?'border-brand text-brand':'border-transparent text-muted')} aria-current={tab==='skills'?'page':undefined} onClick={()=>setTab('skills')}>{en?'Skills':'스킬'}</Button>
  </nav>
  {tab==='plugins'?<PluginCatalog initialCategory={initialCategory}/>:<div className="min-h-0 flex-1 overflow-y-auto"><SkillLibrary/></div>}
 </div>
}
