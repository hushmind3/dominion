import type {ReactNode} from 'react'
import {Button} from '../../components/Primitives'
import {Surface} from '../../components/ui/WorkspaceUI'

/** All prompt scopes share the same field hierarchy and action placement. */
export function PromptSection({id,title,scope,controls,children}:{id:string;title:ReactNode;scope:ReactNode;controls?:ReactNode;children:ReactNode}) {
 return <Surface id={id} className="grid scroll-mt-24 gap-4 [&>p]:text-sm [&>p]:leading-7 [&>p]:text-muted">
  <header className="flex items-center justify-between gap-4"><div className="min-w-0"><span className="text-xs text-muted">{scope}</span><h2 className="mt-1 text-xl font-semibold text-ink">{title}</h2></div>{controls}</header>
  {children}
 </Surface>
}

export function PromptConflict({current,onContinue,onDiscard,english=false}:{current:string;onContinue:()=>void;onDiscard:()=>void;english?:boolean}) {
 return <div className="grid gap-3 rounded-xl border border-warning/25 bg-warning-soft p-4">
  <p className="text-sm font-semibold text-warning">{english?'The saved version changed elsewhere.':'다른 곳에서 원본이 수정되었습니다.'}</p>
  <pre className="max-h-44 overflow-auto whitespace-pre-wrap break-words text-xs leading-6 text-muted">{current}</pre>
  <div className="flex flex-wrap gap-2"><Button variant="secondary" onClick={onContinue}>{english?'Keep editing my draft':'내 초안으로 계속 편집'}</Button><Button variant="ghost" onClick={onDiscard}>{english?'Use saved version':'저장된 내용으로 돌아가기'}</Button></div>
 </div>
}
