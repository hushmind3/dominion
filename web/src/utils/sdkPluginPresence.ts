/** Presence is evidence of recent authenticated traffic, not a process monitor. */
export function sdkPluginPresence(plugin:{status:string;lastSeenAt?:string|null},now:number){
 if(plugin.status!=='enabled')return 'disabled'
 if(!plugin.lastSeenAt)return 'never'
 const seen=Date.parse(plugin.lastSeenAt)
 return Number.isFinite(seen)&&seen<=now&&now-seen<=120000?'recent':'stale'
}
