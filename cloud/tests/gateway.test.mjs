import test from 'node:test'
import assert from 'node:assert/strict'
import gateway from '../src/index.js'

const appOrigin='https://dominion.example'
const endpoint=(path, options={})=>new Request('https://edge.example'+path,options)
const env=(more={})=>({ALLOWED_ORIGIN:appOrigin,DB:{prepare(){throw Error('unexpected DB query')}},...more})
const bearer='A'.repeat(43)

test('health reports configuration without requiring client login or a database',async()=>{
 const response=await gateway.fetch(endpoint('/health'),{})
 assert.equal(response.status,200)
 const body=await response.json()
 assert.equal(body.service,'DOMINION Cloud')
 assert.equal(body.login_configured,false)
})

test('cross-origin browser requests are rejected, known preflights are scoped',async()=>{
 const denied=await gateway.fetch(endpoint('/v1/models',{headers:{Origin:'https://another.example'}}),env())
 assert.equal(denied.status,403)
 const preflight=await gateway.fetch(endpoint('/v1/models',{method:'OPTIONS',headers:{Origin:appOrigin}}),env())
 assert.equal(preflight.status,204)
 assert.equal(preflight.headers.get('access-control-allow-origin'),appOrigin)
 assert.ok(!preflight.headers.get('access-control-allow-origin')?.includes('*'))
})

test('model list, checkout, billing and completions always require a session',async()=>{
 for(const [method,path] of [['GET','/v1/models'],['GET','/v1/me'],['GET','/v1/billing'],
  ['POST','/v1/billing/checkout'],['POST','/v1/chat/completions']]){
  const response=await gateway.fetch(endpoint(path,{method,headers:{Origin:appOrigin}}),env())
  assert.equal(response.status,401,path)
 }
})

test('Google ID-token sign-in fails closed when operator OAuth is not configured',async()=>{
 const response=await gateway.fetch(endpoint('/v1/auth/google',{
  method:'POST',headers:{Origin:appOrigin,'Content-Type':'application/json'},
  body:JSON.stringify({credential:'a'.repeat(50)})
 }),env())
 assert.equal(response.status,503)
})

test('Kakao login issues a secure state cookie; mismatched OAuth state is rejected',async()=>{
 const providerEnv=env({KAKAO_CLIENT_ID:'public-client-id',KAKAO_CLIENT_SECRET:'worker-only-secret'})
 const start=await gateway.fetch(endpoint('/v1/auth/kakao/start',{headers:{Origin:appOrigin}}),providerEnv)
 assert.equal(start.status,302)
 const url=new URL(start.headers.get('location'))
 assert.equal(url.hostname,'kauth.kakao.com')
 assert.equal(url.searchParams.get('client_id'),'public-client-id')
 assert.ok(url.searchParams.get('state')?.length>20)
 const cookie=start.headers.get('set-cookie')||''
 assert.match(cookie,/HttpOnly/)
 assert.match(cookie,/Secure/)
 assert.match(cookie,/SameSite=Lax/)
 const rejected=await gateway.fetch(endpoint('/v1/auth/kakao/callback?state=incorrect&code=code'),providerEnv)
 assert.equal(rejected.status,403)
})

test('logout revokes the hashed bearer session rather than only browser storage',async()=>{
 const seen=[]
 const mock=env({DB:{prepare(sql){
  assert.match(sql,/DELETE FROM sessions WHERE token_hash=\?/)
  return {bind(value){
   seen.push(value)
   return {run:async()=>({success:true})}
  }}
 }}})
 const response=await gateway.fetch(endpoint('/v1/auth/logout',{
  method:'POST',headers:{Origin:appOrigin,Authorization:'Bearer '+bearer}
 }),mock)
 assert.equal(response.status,200)
 assert.deepEqual(await response.json(),{ok:true})
 assert.equal(seen.length,1)
 assert.notEqual(seen[0],bearer)
 assert.match(seen[0],/^[A-Za-z0-9_-]+$/)
 const noToken=await gateway.fetch(endpoint('/v1/auth/logout',{method:'POST'}),mock)
 assert.equal(noToken.status,401)
})

test('free model inference streams through one authenticated Rust-compatible route',async()=>{
 let charged=0
 const data=env({
  AI:{run:async(model,{messages})=>{
   assert.equal(model,'@cf/meta/llama-3.1-8b-instruct-fast')
   assert.equal(messages[0].content,'hello')
   return {response:'free reply'}
  }},
  DB:{prepare(sql){
   return {bind(...params){
    return {
     first:async()=>{
      if(sql.includes('FROM sessions s JOIN accounts'))return {id:'google:fixture',credits:0}
      if(sql.includes('FROM subscriptions'))return null
      if(sql.startsWith('INSERT INTO free_quota')){charged++;return {used:charged}}
      return null
     },
     run:async()=>({success:true})
    }
   }}
  }}
 })
 const listing=await gateway.fetch(endpoint('/v1/models',{headers:{Authorization:'Bearer '+bearer}}),data)
 assert.equal(listing.status,200)
 const available=(await listing.json()).data
 assert.ok(available.some(model=>model.id==='@cf/meta/llama-3.1-8b-instruct-fast'))
 const response=await gateway.fetch(endpoint('/v1/chat/completions',{
  method:'POST',headers:{Authorization:'Bearer '+bearer,'Content-Type':'application/json'},
  body:JSON.stringify({model:'@cf/meta/llama-3.1-8b-instruct-fast',messages:[{role:'user',content:'hello'}],stream:true})
 }),data)
 assert.equal(response.status,200)
 assert.match(response.headers.get('content-type'),/text\/event-stream/)
 const wire=await response.text()
 assert.match(wire,/free reply/)
 assert.match(wire,/data: \[DONE\]/)
 assert.equal(charged,1)
})

test('unprovisioned Worker is unavailable rather than accepting model traffic',async()=>{
 const response=await gateway.fetch(endpoint('/v1/models'),{ALLOWED_ORIGIN:appOrigin})
 assert.equal(response.status,503)
})
