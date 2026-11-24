import {test} from 'node:test'
import assert from 'node:assert/strict'
import {DatabaseSync} from 'node:sqlite'
import {readFileSync} from 'node:fs'
import worker from '../src/index.js'
import {hash,randomToken} from '../src/token_crypto.js'
test('Assist pairing shares account quota, restricts billing, and supports independent revocation',async()=>{
 const db=new DatabaseSync(':memory:');db.exec(readFileSync(new URL('../schema.sql',import.meta.url),'utf8'))
 const DB={prepare(sql){return{bind(...params){const q=db.prepare(sql);return{async first(){return q.get(...params)||null},async all(){return{results:q.all(...params)}},async run(){return{meta:{changes:Number(q.run(...params).changes)}}}}}}}}
 const now=Math.floor(Date.now()/1000),owner=randomToken(),other=randomToken()
 for(const [id,token] of [['google:owner',owner],['google:other',other]]){
  db.prepare('INSERT INTO accounts(id,created_at) VALUES(?,?)').run(id,now)
  db.prepare('INSERT INTO sessions VALUES(?,?,?)').run(await hash(token),id,now+600)
 }
 const env={DB,ALLOWED_ORIGIN:'http://127.0.0.1:4317',FREE_DAILY_LIMIT:'1',AI:{async run(){return {response:'fixture answer'}}}}
 const call=(path,token,body)=>worker.fetch(new Request('https://dominion.example'+path,{headers:{...(token?{authorization:'Bearer '+token}:{}),...(body===undefined?{}:{'content-type':'application/json'})},...(body===undefined?{}:{method:'POST',body:JSON.stringify(body)})}),env)
 const app='dapp_'+randomToken()
 const start=await call('/v1/apps/start',null,{app_name:'DOMINION Assist',token_hash:await hash(app)})
 assert.equal(start.status,200);const pair=await start.json()
 assert.equal(pair.verification_uri,'https://dominion.example/connect?code='+pair.id)
 env.GOOGLE_CLIENT_ID='test.apps.googleusercontent.com'
 const page=await (await call('/connect?code='+pair.id)).text();assert.match(page,/DOMINION Assist/);assert.match(page,/nonce:code/);assert.ok(!page.includes(app))
 assert.equal((await call('/v1/me',app)).status,401)
 assert.equal((await (await call('/v1/apps/status',app)).json()).status,'pending')
 assert.equal((await call('/v1/apps/approve',null,{id:pair.id})).status,401)
 assert.equal((await (await call('/v1/apps/inspect',owner,{id:pair.id})).json()).app_name,'DOMINION Assist')
 assert.equal((await call('/v1/apps/approve',owner,{id:pair.id})).status,200)
 assert.equal((await call('/v1/apps/approve',other,{id:pair.id})).status,409)
 assert.equal((await (await call('/v1/me',app)).json()).account_id,'google:owner')
 assert.equal((await call('/v1/billing',app)).status,403)
 assert.equal((await call('/v1/apps',app)).status,403)
 assert.equal((await call('/v1/apps/revoke',other,{id:pair.id})).status,404)
 assert.equal((await call('/v1/chat/completions',app,{model:'paid-model',messages:[{role:'user',content:'x'}]})).status,403)
 const models=await (await call('/v1/models',app)).json();assert.ok(models.data.every(x=>x.free))
 const model=models.data.find(x=>x.id.startsWith('@cf/')).id
 const body={model,messages:[{role:'user',content:'test'}]}
 assert.equal((await call('/v1/chat/completions',app,body)).status,200)
 assert.equal((await call('/v1/chat/completions',owner,body)).status,429)
 assert.equal((await call('/v1/apps/revoke',owner,{id:pair.id})).status,200)
 assert.equal((await call('/v1/me',app)).status,401)
 assert.equal((await call('/v1/me',owner)).status,200)
 assert.equal(db.prepare('SELECT token_hash FROM app_sessions').get().token_hash,await hash(app))
 db.close()
})
