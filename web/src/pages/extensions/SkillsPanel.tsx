import {del,post} from '../../api'
import {Action,Empty,Panel,StateTag,Subheading} from '../../components/UI'
import {Button,TextInput} from '../../components/Primitives'
import {ArrowRight,BookOpenText,Plus,Search,Trash2} from 'lucide-react'
import {t,useLocale} from '../../i18n'

type Props={
 repo:string;setRepo:(v:string)=>void;repoBusy:boolean;importRepo:()=>Promise<void>;
 skills:any;addSkillTemplate:(t:any)=>Promise<void>;filteredSkills:any[];
 skillCategories:string[];skillCategory:string;setSkillCategory:(v:string)=>void;
 skillQuery:string;setSkillQuery:(v:string)=>void;setSkillEdit:(v:any)=>void;
 setError:(v:string)=>void;setMessage:(v:string)=>void;
}

/** Skill library: a filter rail on the left, template and owned-skill cards
 *  on the right. Both grids share one card shape so the two lists read as one
 *  library instead of two unrelated sections. */
export function SkillsPanel(p:Props){
 useLocale()
 const {repo,setRepo,repoBusy,importRepo,skills,addSkillTemplate,filteredSkills,skillCategories,skillCategory,setSkillCategory,skillQuery,setSkillQuery,setSkillEdit,setError,setMessage}=p
 return (
  <div className="grid gap-6 lg:grid-cols-[200px_minmax(0,1fr)]">
   <aside className="min-w-0">
    <div className="flex flex-col gap-2">
     <TextInput value={repo} onChange={(e)=>setRepo(e.target.value)} placeholder={t('sp.repoPlaceholder')}/>
     <Action onClick={()=>void importRepo()} disabled={repoBusy} className="justify-center">
      {repoBusy?t('sp.importing'):t('sp.import')}
     </Action>
    </div>
    <div className="mt-4 flex items-center gap-2 rounded-lg border border-divider bg-surface px-3 py-2">
     <Search size={16} className="shrink-0 text-muted"/>
     <TextInput value={skillQuery} onChange={(e)=>setSkillQuery(e.target.value)} placeholder={t('sp.searchPlaceholder')}/>
    </div>
    <nav aria-label={t('sp.templates')} className="mt-4 flex flex-wrap gap-1 lg:flex-col">
     {skillCategories.map(category=><Button key={category} variant={skillCategory===category?'primary':'ghost'} onClick={()=>setSkillCategory(category)} className="lg:w-full lg:justify-start !px-3 !py-1.5 !text-[12px]">{category==='all'?t('sp.all'):category}</Button>)}
    </nav>
   </aside>
   <div className="min-w-0">
    <Subheading title={t('sp.templates')} extra={<span className="text-[11px] text-muted">{t('sp.count',{count:filteredSkills.length})}</span>}/>
    <div className="mb-6 grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
     {filteredSkills.map(template=>{
      const exists=skills.value.some((x:any)=>x.name===template.name)
      return <Panel key={template.name} className="flex min-w-0 flex-col gap-3 p-4">
       <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand"><BookOpenText size={20}/></div>
       <h3 className="text-[13px] font-semibold text-ink">{template.name}</h3>
       <p className="line-clamp-3 text-[12px] leading-5 text-muted">{template.instructions}</p>
       <div className="mt-auto border-t border-divider pt-3">
        {exists
         ?<Button variant="secondary" className="w-full justify-center" onClick={async()=>{const target=skills.value.find((x:any)=>x.name===template.name);if(!target)return;await del('/api/skills/'+target.id);await skills.reload()}}>{t('sp.cancel')}</Button>
         :<Button variant="secondary" className="w-full justify-center" onClick={()=>void addSkillTemplate(template)}>{t('sp.install')}</Button>}
       </div>
      </Panel>
     })}
    </div>
    <Subheading title={t('sp.mine')} extra={<Action onClick={()=>setSkillEdit({name:'',instructions:'',enabled:true})}><Plus size={16}/> {t('sp.write')}</Action>}/>
    <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
     {skills.value.map((x:any)=><Panel key={x.id} className="flex min-w-0 flex-col gap-3 p-4">
      <div className="flex items-start justify-between gap-2">
       <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand"><BookOpenText size={20}/></div>
       <StateTag online={x.enabled}>{x.enabled?t('sp.active'):t('sp.suggested')}</StateTag>
      </div>
      <h3 className="text-[13px] font-semibold text-ink">
       {x.name}
       {String(x.source||'').includes('anthropics')&&<span className="ml-2 rounded bg-brand-soft px-1.5 py-0.5 text-[10px] font-semibold text-brand-hover">{t('sp.claudeBasis')}</span>}
      </h3>
      <p className="line-clamp-3 text-[12px] leading-5 text-muted">{x.instructions.slice(0,170)}</p>
      <div className="mt-auto flex flex-wrap items-center gap-1.5 border-t border-divider pt-3">
       <Button variant={x.enabled?'ghost':'primary'} onClick={async()=>{try{await post('/api/skills',{...x,enabled:!x.enabled});skills.reload()}catch(e){setError((e as Error).message)}}}>{x.enabled?t('sp.disable'):t('sp.enable')}</Button>
       <a href={'/api/skills/'+x.id+'/export'} download={x.name+'.md'} className="inline-flex min-h-8 items-center gap-1 rounded-lg border border-divider px-2 py-1 text-[11px] font-semibold text-ink hover:bg-canvas">{t('sp.export')}</a>
       <Button onClick={()=>setSkillEdit(x)}>{t('sp.edit')} <ArrowRight size={15}/></Button>
       <Button size="icon" variant="ghost" title={t('sp.delete')} onClick={async()=>{if(!confirm(t('sp.confirmDelete',{name:x.name})))return;try{await del('/api/skills/'+x.id);await skills.reload();setMessage(t('ext.skillDeleted',{name:x.name}))}catch(e){setError((e as Error).message)}}}><Trash2 size={16}/></Button>
      </div>
     </Panel>)}
     {!skills.value.length&&<Panel className="md:col-span-2 2xl:col-span-3"><Empty icon={BookOpenText} title={t('sp.empty')} description={t('sp.emptyDesc')}/></Panel>}
    </div>
   </div>
  </div>
 )
}
