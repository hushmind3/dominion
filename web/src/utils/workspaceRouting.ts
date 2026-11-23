/** Canonical navigation for the consolidated three management destinations.
 * Redirect historical deep links rather than keeping duplicate visible pages. */
export function canonicalWorkspaceHash(hash:string):string{
 const [name,rawQuery]=hash.replace(/^#\/?/, '').split('?')
 const params=new URLSearchParams(rawQuery)
 if(name==='agents'||name==='prompts'){
  const view=name==='agents'?'personas':params.get('view')||'shared'
  params.set('view',view==='personas'||view==='groups'?view:'shared')
  return '#/memory?'+params.toString()
 }
 if(name==='models')return '#/settings?view=ai'
 if(name==='capabilities')return '#/settings?view=features'
 if(name==='home')return '#/settings?view=overview'
 if(name==='activity')return '#/settings?view=activity'
 if(name==='extensions'){
  const tab=params.get('tab')||params.get('section')
  const view=tab==='market'?'registry':tab==='plugins'||tab==='skills'?tab:'mcp'
  return '#/connections?view='+view
 }
 return hash
}
