/** Shared account gateway, independent of the local Rust SDK. No operator key. */
export const DOMINION_CLOUD_URL='https://dominion-cloud.dominion-hushmind.workers.dev';
export type CloudPairing={id:string;token:string;expires_in:number;status:'pending';verification_uri:string};
export type CloudAppStatus={id:string;status:'pending'|'approved'|'revoked'|'expired';expires_at:number};
export type CloudModel={id:string;name:string;free:boolean};
export class DominionCloudClient {
 constructor(private readonly token?:string,private readonly fetcher:typeof fetch=fetch){}
 private async request<T>(path:string,body?:unknown):Promise<T>{
  const r=await this.fetcher(DOMINION_CLOUD_URL+path,{redirect:'error',cache:'no-store',headers:{...(this.token?{Authorization:'Bearer '+this.token}:{}),...(body===undefined?{}:{'content-type':'application/json'})},...(body===undefined?{}:{method:'POST',body:JSON.stringify(body)})});
  const result=await r.json() as T&{error?:string};
  if(!r.ok)throw new Error(result.error||'Cloud HTTP '+r.status);
  return result;
 }
 /** Persist the returned app token in the application's private credential store.
  * Display only id to the user, who approves in their DOMINION Cloud account. */
 static async begin(appName:string,fetcher:typeof fetch=fetch):Promise<CloudPairing>{
  const b64=(bytes:Uint8Array)=>btoa(String.fromCharCode(...bytes)).replace(/=/g,'').replace(/\+/g,'-').replace(/\//g,'_');
  const token='dapp_'+b64(crypto.getRandomValues(new Uint8Array(32)));
  const token_hash=b64(new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(token))));
  const pair=await new DominionCloudClient(undefined,fetcher).request<Omit<CloudPairing,'token'>>('/v1/apps/start',{app_name:appName,token_hash});
  return {...pair,token};
 }
 status(){return this.request<CloudAppStatus>('/v1/apps/status')}
 me(){return this.request<{account_id:string;credits_micro_krw:number;scope:'free_inference';app_session_id:string}>('/v1/me')}
 billing(){return this.request<{account_id:string;credits_micro_krw:number;plan:'free'|'pro';valid_until:number|null;free_daily_limit:number;free_daily_used:number;free_daily_remaining:number;free_monthly_limit:number|null;free_monthly_remaining:number|null}>('/v1/billing')}
 models(){return this.request<{data:CloudModel[]}>('/v1/models')}
 complete(input:{model:string;messages:{role:'system'|'user'|'assistant';content:string}[];max_tokens?:number}){
  return this.request<{choices:{message:{role:string;content:string};finish_reason:string}[]}>('/v1/chat/completions',{...input,stream:false});
 }
 disconnect(){return this.request<{status:'revoked'}>('/v1/apps/disconnect',{})}
}
