import test from 'node:test';import assert from 'node:assert/strict';import {seal,unseal,readGoogle,googleRoute} from '../src/assistant_google.js';
test('refresh token ciphertext is bound to account',async()=>{const e={CONNECTOR_TOKEN_SECRET:'x'.repeat(40)},x=await seal(e,'google:a','refresh-secret');assert.ok(!x.includes('refresh-secret'));assert.equal(await unseal(e,'google:a',x),'refresh-secret');await assert.rejects(unseal(e,'google:b',x))});
test('Gmail metadata result uses only approved Google endpoints',async()=>{let calls=[];const send=async(url,options)=>{calls.push(url);assert.equal(options.headers.authorization,'Bearer access');return Response.json(calls.length===1?{messages:[{id:'m'}]}:{payload:{headers:[{name:'Subject',value:'hello'},{name:'From',value:'sender'},{name:'Date',value:'today'}]}})};assert.deepEqual(await readGoogle('gmail_recent','access',send),[{subject:'hello',from:'sender',date:'today'}]);assert.equal(calls.length,2);assert.ok(calls.every(u=>u.startsWith('https://gmail.googleapis.com/')));await assert.rejects(readGoogle('arbitrary_url','access',send));});
test('not configured and wrong app scope fail before DB access',async()=>{const req=new Request('https://example.com/v1/assistant/google/status');assert.equal((await googleRoute(req,{},{})).status,503);const env={DB:{},ASSISTANT_GOOGLE_ENABLED:'1',CONNECTOR_GOOGLE_CLIENT_ID:'client',CONNECTOR_GOOGLE_CLIENT_SECRET:'secret',CONNECTOR_TOKEN_SECRET:'x'.repeat(40)};assert.equal((await googleRoute(req,env,{account:async()=>({id:'google:a',scope:'app',capability:'inference'})})).status,403)});

import {DatabaseSync} from 'node:sqlite';
import {readFileSync} from 'node:fs';
test('OAuth lifecycle: one-time callback, refresh and real operation shape, disconnect blocks stale callback',async()=>{
 const db=new DatabaseSync(':memory:');db.exec(readFileSync(new URL('../schema.sql',import.meta.url),'utf8'));
 const DB={prepare(sql){return{bind(...a){return{first:async()=>db.prepare(sql).get(...a)||null,all:async()=>({results:db.prepare(sql).all(...a)}),run:async()=>({meta:{changes:Number(db.prepare(sql).run(...a).changes)}})}}}}};
 const now=Math.floor(Date.now()/1000);
 db.prepare('INSERT INTO accounts(id,created_at) VALUES(?,?)').run('google:owner',now);
 db.prepare("INSERT INTO app_sessions(id,token_hash,app_name,capability,ip_hash,status,account_id,created_at,expires_at) VALUES(?,?,?,?,?,'approved',?,?,?)").run('session','hash','assistant','assistant_remote','ip','google:owner',now,now+600);
 const env={DB,ASSISTANT_GOOGLE_ENABLED:'1',CONNECTOR_GOOGLE_CLIENT_ID:'client',CONNECTOR_GOOGLE_CLIENT_SECRET:'secret',CONNECTOR_TOKEN_SECRET:'x'.repeat(40)};
 const account=async()=>({id:'google:owner',scope:'app',capability:'assistant_remote',app_session_id:'session'});
 let subject='owner',race=false;
 const send=async(url,options)=>{
  if(url==='https://oauth2.googleapis.com/token'){
   if(options.body.get('grant_type')==='authorization_code'){
    assert.ok(options.body.get('code_verifier'));
    if(race)await call('/disconnect',{});
    return Response.json({access_token:'access',refresh_token:'refresh',scope:'openid https://www.googleapis.com/auth/calendar.events.readonly'});
   }
   assert.equal(options.body.get('refresh_token'),'refresh');return Response.json({access_token:'access'});
  }
  if(url==='https://openidconnect.googleapis.com/v1/userinfo')return Response.json({sub:subject});
  if(url==='https://oauth2.googleapis.com/revoke')return new Response(null,{status:200});
  if(url.startsWith('https://www.googleapis.com/calendar/'))return Response.json({items:[{summary:'약속',start:{date:'2027-01-01'}}]});
  throw Error('unexpected URL');
 };
 const call=(path,body)=>googleRoute(new Request('https://dominion.example/v1/assistant/google'+path,body===undefined?{}:{method:'POST',body:JSON.stringify(body)}),env,{account,send});
 async function start(){const r=await call('/start',{});assert.equal(r.status,200);const u=new URL((await r.json()).url);assert.equal(u.searchParams.get('code_challenge_method'),'S256');return '/callback?code=test&state='+u.searchParams.get('state')}
 let cb=await start();assert.equal((await call(cb)).status,200);assert.equal((await call(cb)).status,403);
 assert.deepEqual((await (await call('/status')).json()).services,['calendar']);
 assert.equal((await call('/run',{operation:'gmail_recent'})).status,409);
 assert.deepEqual((await (await call('/run',{operation:'calendar_upcoming'})).json()).items,[{title:'약속',start:'2027-01-01',end:'',location:''}]);
 assert.equal((await call('/disconnect',{})).status,200);
 assert.equal((await call('/run',{operation:'calendar_upcoming'})).status,409);
 subject='another';cb=await start();assert.equal((await call(cb)).status,403);assert.equal((await (await call('/status')).json()).connected,false);
 subject='owner';race=true;cb=await start();assert.equal((await call(cb)).status,403);assert.equal((await (await call('/status')).json()).connected,false);
});

test('durable job survives missing authorization then reads once after grant',async()=>{
 const {drainGoogleJobs}=await import('../src/assistant_google.js');
 const db=new DatabaseSync(':memory:');db.exec(readFileSync(new URL('../schema.sql',import.meta.url),'utf8'));
 const DB={prepare(sql){return{bind(...a){return{first:async()=>db.prepare(sql).get(...a)||null,all:async()=>({results:db.prepare(sql).all(...a)}),run:async()=>({meta:{changes:Number(db.prepare(sql).run(...a).changes)}})}}}}};
 const ts=Math.floor(Date.now()/1000);db.prepare('INSERT INTO accounts(id,created_at) VALUES(?,?)').run('google:owner',ts);db.prepare("INSERT INTO app_sessions(id,token_hash,app_name,capability,ip_hash,status,account_id,created_at,expires_at) VALUES(?,?,?,?,?,'approved',?,?,?)").run('session','hash','assistant','assistant_remote','ip','google:owner',ts,ts+600);
 const env={DB,ASSISTANT_GOOGLE_ENABLED:'1',CONNECTOR_GOOGLE_CLIENT_ID:'client',CONNECTOR_GOOGLE_CLIENT_SECRET:'secret',CONNECTOR_TOKEN_SECRET:'x'.repeat(40)};
 const account=async()=>({id:'google:owner',scope:'app',capability:'assistant_remote',app_session_id:'session'});let reads=0;
 const send=async(url)=>{if(url.includes('oauth2.googleapis.com/token'))return Response.json({access_token:'access'});if(url.includes('/calendar/')){reads++;return Response.json({items:[{summary:'병원',start:{date:'2027-01-01'}}]})}throw Error('unexpected')};
 const call=(path,b)=>googleRoute(new Request('https://example.com/v1/assistant/google/jobs'+path,b===undefined?{}:{method:'POST',body:JSON.stringify(b)}),env,{account,send});
 const payload={id:'request-123',operation:'calendar_upcoming'};assert.equal((await (await call('',payload)).json()).status,'awaiting_auth');
 await drainGoogleJobs(env,null,send);assert.equal(reads,0);
 db.prepare('INSERT INTO assistant_google_grants VALUES(?,?,?,?)').run('google:owner',await seal(env,'google:owner','refresh'),JSON.stringify(['calendar']),ts);
 await drainGoogleJobs(env,null,send);await drainGoogleJobs(env,null,send);assert.equal(reads,1);
 const row=(await (await call('')).json()).jobs[0];assert.equal(row.status,'completed');assert.equal(row.items[0].title,'병원');
 assert.equal((await (await call('',payload)).json()).status,'completed');
 assert.ok(!db.prepare('SELECT result_cipher FROM assistant_google_jobs').get().result_cipher.includes('병원'));
});
