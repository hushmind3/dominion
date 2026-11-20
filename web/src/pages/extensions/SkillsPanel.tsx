import {del,post} from '../../api'
import {Action,Empty,Panel,StateTag,Subheading} from '../../components/UI'
import {Button,TextInput} from '../../components/Primitives'
import {ArrowRight,BookOpenText,Check,Plus,Search,Trash2} from 'lucide-react'
import {t,useLocale} from '../../i18n'
import {useState} from 'react'

type Props={
 repo:string;setRepo:(v:string)=>void;repoBusy:boolean;importRepo:()=>Promise<void>;
 skills:any;addSkillTemplate:(t:any)=>Promise<void>;filteredSkills:any[];
 skillCategories:string[];skillCategory:string;setSkillCategory:(v:string)=>void;
 skillQuery:string;setSkillQuery:(v:string)=>void;setSkillEdit:(v:any)=>void;
 setError:(v:string)=>void;setMessage:(v:string)=>void;
}

export function SkillsPanel(p:Props){
 useLocale()
 const [busy,setBusy]=useState<string|null>(null)
 const {repo,setRepo,repoBusy,importRepo,skills,addSkillTemplate,filteredSkills,skillCategories,skillCategory,setSkillCategory,skillQuery,setSkillQuery,setSkillEdit,setError,setMessage}=p
 const owned=skills.value.filter((item:any)=>
  [item.name,item.instructions,item.source].join(' ').toLowerCase().includes(skillQuery.trim().toLowerCase()))
 return (
  <>
    <Panel className="mb-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <TextInput
          className="!w-auto min-w-[200px] flex-1"
          aria-label={t('sp.repoPlaceholder')}
          value={repo}
          onChange={(e) => setRepo(e.target.value)}
          onKeyDown={e=>{if(e.key==='Enter'&&repo.trim()&&!repoBusy)void importRepo()}}
          placeholder={t('sp.repoPlaceholder')}
        />
        <Action onClick={() => void importRepo()} disabled={repoBusy||!repo.trim()}>
          {repoBusy ? t('sp.importing') : t('sp.import')}
        </Action>
      </div>
    </Panel>
    <Subheading
      title={t('sp.templates')}
      extra={
        <span className="text-[11px] text-[#8A8176]">
          {t('sp.count', { count: filteredSkills.length })}
        </span>
      }
    />
    <div className="mb-4 flex flex-wrap items-center gap-1.5">
      {skillCategories.map((category) => (
        <Button
          key={category}
          variant={skillCategory === category ? "primary" : "ghost"}
          onClick={() => setSkillCategory(category)}
          className="!px-3 !py-1.5 !text-[12px]"
        >
          {category === "all" ? t('sp.all') : category}
        </Button>
      ))}
    </div>
    <div className="mb-5 flex items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2">
      <Search size={17} className="text-[#A79C8E]" />
      <TextInput
        aria-label={t('sp.searchPlaceholder')}
        className="!border-0 !bg-transparent focus:!ring-0"
        value={skillQuery}
        onChange={(e) => setSkillQuery(e.target.value)}
        placeholder={t('sp.searchPlaceholder')}
      />
    </div>
    <div className="mb-6 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
      {filteredSkills.map((template) => {
        const installed = skills.value.find((x:any) => x.name === template.name);
        return (
          <Panel
            key={template.name}
            className="flex min-w-0 flex-col gap-3 p-5"
          >
            <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
              <BookOpenText size={20} />
            </div>
            <h3 className="break-words text-sm font-bold">{template.name}</h3>
            <p className="line-clamp-4 break-words text-xs leading-5 text-[#7C7469]">{template.instructions}</p>
            <div className="mt-auto border-t border-[#EFE9DF] pt-3">
              {installed ? (
                <StateTag online={installed.enabled}>
                  {installed.enabled?t('sp.active'):t('sp.suggested')}
                </StateTag>
              ) : (
                <Button variant="secondary" disabled={busy!==null} onClick={async()=>{
                 setBusy(template.name);setError('')
                 try{await addSkillTemplate(template)}
                 finally{setBusy(null)}
                }}>{t('sp.install')}</Button>
              )}
            </div>
          </Panel>
        );
      })}
    </div>
    <Subheading
      title={t('sp.mine')}
      extra={
        <Action
          onClick={() =>
            setSkillEdit({ name: "", instructions: "", enabled: true })
          }
        >
          <Plus size={16} /> {t('sp.write')}
        </Action>
      }
    />
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
      {owned.map((x:any) => (
        <Panel key={x.id} className="flex min-w-0 flex-col gap-3 p-5">
          <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
            <BookOpenText size={21} />
          </div>
          <h3 className="break-words text-sm font-bold">
            {x.name}
            {String(x.source || "").includes("anthropics") && (
              <span className="ml-2 rounded bg-[#FFF3E8] px-1.5 py-0.5 text-[10px] font-semibold text-[#A94E30]">
                {t('sp.claudeBasis')}
              </span>
            )}
          </h3>
          <p className="line-clamp-4 break-words text-xs leading-5 text-[#7C7469]">{String(x.instructions||'').slice(0,320)}</p>
          <div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3">
            <StateTag online={x.enabled}>
              {x.enabled ? t('sp.active') : t('sp.suggested')}
            </StateTag>
            <div className="flex items-center gap-1">
              <Button
                variant={x.enabled ? "ghost" : "primary"}
                disabled={busy!==null}
                onClick={async () => {
                  setBusy(x.id);setError('')
                  try {
                    await post("/api/skills", {...x,enabled: !x.enabled});
                    await skills.reload();
                  } catch (e) {
                    setError((e as Error).message);
                  } finally {setBusy(null)}
                }
              >
                {x.enabled ? t('sp.disable') : t('sp.enable')}
              </Button>
              <a
                href={"/api/skills/" + x.id + "/export"}
                download={x.name + ".md"}
                className="inline-flex items-center gap-1 rounded-lg border border-[#E1D9CC] px-2 py-1 text-[11px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]"
              >
                {t('sp.export')}
              </a>
              <Button onClick={() => setSkillEdit(x)}>
                {t('sp.edit')} <ArrowRight size={15} />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                title={t('sp.delete')}
                aria-label={t('sp.delete')+' '+x.name}
                disabled={busy!==null}
                onClick={async () => {
                  if (!confirm(t('sp.confirmDelete', { name: x.name }))) return;
                  setBusy(x.id);setError('')
                  try {
                    await del("/api/skills/" + x.id);
                    await skills.reload();
                    setMessage(t('ext.skillDeleted', { name: x.name }));
                  } catch (e) {
                    setError((e as Error).message);
                  } finally {setBusy(null)}
                }}
              >
                <Trash2 size={16} />
              </Button>
            </div>
          </div>
        </Panel>
      ))}
      {!owned.length && (
        <Panel>
          <Empty
            icon={BookOpenText}
            title={t('sp.empty')}
            description={t('sp.emptyDesc')}
          />
        </Panel>
      )}
    </div>
  </>
 )
}
