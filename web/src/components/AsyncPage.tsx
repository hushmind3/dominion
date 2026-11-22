import {Suspense, type ReactNode} from 'react'
import {PageLoadBoundary} from './PageLoadBoundary'

export function AsyncPage({children, label}: {children: ReactNode; label: string}) {
  return <PageLoadBoundary><Suspense fallback={<div className="workspace-loading" role="status" aria-label={label}>
    <span className="workspace-loading-mark">D</span><p>{label}</p><div aria-hidden="true"><i/><i/><i/></div>
  </div>}>{children}</Suspense></PageLoadBoundary>
}
