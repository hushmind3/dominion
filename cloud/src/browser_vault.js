// Private, account/site-bound browser data. Never exposed to a planning model.
const enc=new TextEncoder(),dec=new TextDecoder();
export const MAX_PDF=2*1024*1024, PROFILE_MS=24*3600*1000, DOCUMENT_MS=7*24*3600*1000;
export const vaultReady=env=>typeof env.BROWSER_VAULT_SECRET==='string'&&env.BROWSER_VAULT_SECRET.length>=32;
const encode=bytes=>{let out='';for(let i=0;i<bytes.length;i+=8192)out+=String.fromCharCode(...bytes.subarray(i,i+8192));return btoa(out)};
const decode=s=>Uint8Array.from(atob(s),x=>x.charCodeAt(0));
const key=async env=>crypto.subtle.importKey('raw',await crypto.subtle.digest('SHA-256',enc.encode('dominion-browser-v1:'+env.BROWSER_VAULT_SECRET)),{name:'AES-GCM'},false,['encrypt','decrypt']);
export async function protect(env,scope,value){if(!vaultReady(env))throw Error('비공개 저장소가 설정되지 않았습니다.');const iv=crypto.getRandomValues(new Uint8Array(12));const cipher=await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData:enc.encode(scope)},await key(env),enc.encode(JSON.stringify(value)));return encode(iv)+'.'+encode(new Uint8Array(cipher))}
export async function reveal(env,scope,value){const [iv,cipher]=value.split('.');return JSON.parse(dec.decode(await crypto.subtle.decrypt({name:'AES-GCM',iv:decode(iv),additionalData:enc.encode(scope)},await key(env),decode(cipher))))}
const allowedHost=(host,root)=>host===root||host.endsWith('.'+root);
export function reusableCookies(rows,root,time=Date.now()){
 if(!Array.isArray(rows))return [];
 const result=[];let bytes=0;
 for(const c of rows){
  if(!c||typeof c.name!=='string'||typeof c.value!=='string'||typeof c.domain!=='string'||typeof c.path!=='string'||!c.secure||c.partitionKey||c.partitionKeyOpaque||c.name.length>256||c.value.length>4096||c.path.length>1024||!c.path.startsWith('/'))continue;
  const host=c.domain.replace(/^\./,'').toLowerCase();if(!allowedHost(host,root)||!/^[-a-z0-9.]+$/.test(host))continue;
  if(Number.isFinite(c.expires)&&c.expires>0&&c.expires*1000<=time)continue;
  // Host-only cookies remain host-only. Never broaden the original domain.
  const p={name:c.name,value:c.value,url:'https://'+host+'/',path:c.path,secure:true,httpOnly:!!c.httpOnly};
  if(c.domain.startsWith('.'))p.domain=c.domain;
  if(['Strict','Lax','None'].includes(c.sameSite))p.sameSite=c.sameSite;
  if(Number.isFinite(c.expires)&&c.expires>0)p.expires=Math.min(c.expires,(time+PROFILE_MS)/1000);
  const size=JSON.stringify(p).length;if(bytes+size>24000||result.length>=100)break;bytes+=size;result.push(p);
 }return result;
}
export function pdfBytes(base64){
 if(typeof base64!=='string'||base64.length>Math.ceil(MAX_PDF/3)*4||!/^[A-Za-z0-9+/]+={0,2}$/.test(base64))throw Error('PDF 파일 크기 또는 형식이 지원 범위를 벗어났습니다.');
 const bytes=decode(base64);if(bytes.length<8||bytes.length>MAX_PDF||dec.decode(bytes.subarray(0,5))!=='%PDF-')throw Error('실제 PDF 파일이 아닙니다.');return bytes;
}
export const sha256=async bytes=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),x=>x.toString(16).padStart(2,'0')).join('');
export async function storeDocument(storage,env,j,data,kind,source){
 const bytes=pdfBytes(data),id=crypto.randomUUID(),scope='document:'+j.account+':'+j.id+':'+id;
 const meta={id,name:(kind==='original'?'original-':'page-')+id.slice(0,8)+'.pdf',kind,mime:'application/pdf',size:bytes.length,sha256:await sha256(bytes),source,expiresAt:Date.now()+DOCUMENT_MS};
 const cipher=await protect(env,scope,data),chunks=cipher.match(/.{1,48000}/g)||[];
 const values=Object.fromEntries(chunks.map((x,i)=>['doc:'+id+':'+i,x]));
 // Atomic batch of bounded values; the public metadata never contains file bytes.
 await storage.put({...values,['docmeta:'+id]:{...meta,account:j.account,jobId:j.id,chunks:chunks.length}});
 return meta;
}
export async function loadDocument(storage,env,account,jobId,id){
 const m=await storage.get('docmeta:'+id);if(!m||m.account!==account||m.jobId!==jobId||Date.now()>=m.expiresAt)return null;
 const chunks=await Promise.all(Array.from({length:m.chunks},(_,i)=>storage.get('doc:'+id+':'+i)));
 if(chunks.some(x=>typeof x!=='string'))throw Error('저장된 문서가 손상되었습니다.');
 const data=await reveal(env,'document:'+account+':'+jobId+':'+id,chunks.join('')),bytes=pdfBytes(data);
 if(bytes.length!==m.size||await sha256(bytes)!==m.sha256)throw Error('문서 무결성 검증 실패');
 const {account:_a,jobId:_j,chunks:_c,...meta}=m;return {...meta,base64:data};
}
export async function removeDocument(storage,id){
 const m=await storage.get('docmeta:'+id);if(m)await storage.delete(['docmeta:'+id,...Array.from({length:m.chunks},(_,i)=>'doc:'+id+':'+i)]);
}
export async function pruneVault(storage){
 const records=await storage.list({prefix:'docmeta:'}),docs=[...records.values()].filter(x=>x?.id&&Number.isFinite(x.expiresAt)).sort((a,b)=>b.expiresAt-a.expiresAt);
 for(const [i,m] of docs.entries())if(i>=5||m.expiresAt<=Date.now())await removeDocument(storage,m.id);
 const profiles=await storage.list({prefix:'profile:'});for(const [key,p] of profiles)if(p?.expiresAt<=Date.now())await storage.delete(key);
}
