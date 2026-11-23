/** Shared entry to the existing local runtime installer; no duplicate installer. */
export const localRuntimeSetupHref = '#/models?tab=local&view=import'

/** Validate the hash parameters before using them as a Model Hub landing view. */
export function modelHubLanding(hash: string): {
  tab: 'providers' | 'local'
  localView: 'installed' | 'search' | 'import'
} {
  const [pathname, query = ''] = hash.replace(/^#\/?/, '').split('?')
  if (pathname !== 'models') return {tab: 'providers', localView: 'installed'}
  const params = new URLSearchParams(query)
  if (params.get('tab') !== 'local') return {tab: 'providers', localView: 'installed'}
  const requestedView = params.get('view')
  return {
    tab: 'local',
    localView: requestedView === 'import' || requestedView === 'search'
      ? requestedView : 'installed',
  }
}
