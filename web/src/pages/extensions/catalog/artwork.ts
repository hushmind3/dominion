import {apiFetch} from '../../../api/transport'
// Public artwork may be shared between cards. Fetch through the authenticated
// app transport; data URLs also satisfy the desktop's existing image CSP.
const cache=new Map<string,Promise<string>>()
export function loadArtwork(source:string):Promise<string>{
 let pending=cache.get(source)
 if(!pending){
  pending=apiFetch(source).then(async response=>{if(!response.ok)throw Error('Artwork unavailable');const blob=await response.blob();return new Promise<string>((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>resolve(String(reader.result));reader.onerror=()=>reject(reader.error);reader.readAsDataURL(blob)})})
  if(cache.size>=256){const oldest=cache.keys().next().value;if(oldest)cache.delete(oldest)}
  cache.set(source,pending)
  void pending.catch(()=>{if(cache.get(source)===pending)cache.delete(source)})
 }
 return pending
}
