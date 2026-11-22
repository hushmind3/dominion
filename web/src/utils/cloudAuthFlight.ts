/** DOMINION Cloud Google sign-in may outlive a React page or navigation. */
export const nativeGoogleFlightKey='dominion_cloud_google_native_flight'
export const nativeGoogleFlightTtlMs=5*60*1000

export function encodeNativeGoogleFlight(id:string,now=Date.now()):string {
 return JSON.stringify({id,expiresAt:now+nativeGoogleFlightTtlMs})
}

export function decodeNativeGoogleFlight(raw:string|null,now=Date.now()):string {
 if(!raw)return ''
 try{
  const flight=JSON.parse(raw) as {id?:unknown;expiresAt?:unknown}
  if(typeof flight?.id!=='string'||!/^[A-Za-z0-9_-]{43}$/.test(flight.id))return ''
  if(typeof flight.expiresAt!=='number'||!Number.isFinite(flight.expiresAt))return ''
  return flight.expiresAt>now?flight.id:''
 }catch{return ''}
}
