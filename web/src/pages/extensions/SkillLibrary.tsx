import {useState} from 'react'
import {get,post,del} from '../../api'
import {Button,TextInput,TextArea} from '../../components/Primitives'
import {Notice,useRemote,FormField} from '../../components/UI'
import {useLocale} from '../../i18n'
type Skill={id:string;name:string;instructions:string;enabled:boolean;source?:string;status?:string}
type CatalogSkill={id:string;name:string;description:string;source:string}
export function SkillLibrary(){
 const en=useLocale()==='en',owned=useRemote<Skill[]>('/api/skills',[]),catalog=useRemote<{items:CatalogSkill[]}>('/api/skill-catalog',{items:[]})
 const [query,setQuery]=useState(''),[repo,setRepo]=useState(''),[editing,setEditing]=useState<Partial<Skill>>(),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const act=async(fn:()=>Promise<unknown>)=>{if(busy)return;setBusy(true);setError('');try{await fn();await Promise.all([owned.reload(),catalog.reload()])}catch(e){setError((e as Error).message)}finally{setBusy(false)}}
 const needle=query.trim().toLowerCase()
 return <section className="p-8" aria-label={en?'Skills':'스킬'}><div className="mb-6 flex gap-3"><TextInput aria-label={en?'Search skills':'스킬 검색'} value={query} onChange={e=>setQuery(e.target.value)}/><Button onClick={()=>setEditing({name:'',instructions:'',enabled:false})}>{en?'Create skill':'스킬 만들기'}</Button></div>
  <Notice message={error||owned.error||catalog.error}/>
  {editing?<div className="mb-6 grid gap-4 rounded-2xl border border-divider p-5"><FormField label={en?'Name':'이름'}><TextInput value={editing.name||''} onChange={e=>setEditing({...editing,name:e.target.value})}/></FormField><FormField label={en?'Instructions':'작업 지침'}><TextArea rows={10} value={editing.instructions||''} onChange={e=>setEditing({...editing,instructions:e.target.value})}/></FormField><div className="flex gap-2"><Button disabled={busy||!editing.name||!editing.instructions} onClick={()=>void act(async()=>{await post('/api/skills',editing);setEditing(undefined)})}>{en?'Save':'저장'}</Button><Button variant="ghost" onClick={()=>setEditing(undefined)}>{en?'Close':'닫기'}</Button></div></div>:null}
  <div className="mb-6 flex gap-2"><TextInput aria-label={en?'Skill repository':'스킬 저장소'} value={repo} onChange={e=>setRepo(e.target.value)} placeholder="https://github.com/owner/repository"/><Button variant="secondary" disabled={busy||!repo.trim()} onClick={()=>void act(()=>post('/api/skills/import-repo',{repo:repo.trim()}))}>{en?'Import':'가져오기'}</Button></div>
  <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-4">{owned.value.filter(skill=>[skill.name,skill.instructions].join(' ').toLowerCase().includes(needle)).map(skill=><article key={skill.id} className="rounded-2xl border border-divider bg-white p-5"><h3 className="font-bold">{skill.name}</h3><p className="my-3 line-clamp-3 text-sm text-muted">{skill.instructions}</p><div className="flex flex-wrap gap-2"><Button disabled={busy} onClick={()=>void act(()=>post('/api/skills',{...skill,enabled:!skill.enabled,status:!skill.enabled?'approved':'draft'}))}>{skill.enabled?(en?'Disable':'비활성화'):(en?'Enable':'활성화')}</Button><Button variant="ghost" onClick={()=>setEditing(skill)}>{en?'Settings':'설정'}</Button><Button variant="ghost" disabled={busy} onClick={()=>void act(()=>del('/api/skills/'+encodeURIComponent(skill.id)))}>{en?'Remove':'제거'}</Button></div></article>)}
  {catalog.value.items.filter(item=>!owned.value.some(skill=>skill.name===item.name)&&[item.name,item.description,item.source].join(' ').toLowerCase().includes(needle)).map(item=><article key={item.id} className="rounded-2xl border border-divider bg-white p-5"><h3 className="font-bold">{item.name}</h3><p className="my-3 text-sm text-muted">{item.description}</p><Button disabled={busy} onClick={()=>void act(()=>post('/api/skill-catalog/import',{id:item.id}))}>{en?'Add':'추가'}</Button></article>)}</div>
 </section>
}
