import {Suspense, type ReactNode} from 'react'
import {LoaderCircle} from 'lucide-react'
import {PageLoadBoundary} from './PageLoadBoundary'

export function AsyncPage({children,label}:{children:ReactNode;label:string}) {
 return <PageLoadBoundary><Suspense fallback={<div className="flex h-full min-h-60 flex-col items-center justify-center gap-4 text-sm text-muted" role="status" aria-label={label}><LoaderCircle size={24} className="animate-spin text-brand motion-reduce:animate-none"/><p>{label}</p></div>}>{children}</Suspense></PageLoadBoundary>
}
