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

const paymentRequest=(orderId,paymentKey,amount=3900)=>endpoint('/v1/billing/confirm',{
 method:'POST',headers:{Origin:appOrigin,Authorization:'Bearer '+bearer,'Content-Type':'application/json'},
 body:JSON.stringify({orderId,paymentKey,amount})
})
function paymentFixture(status='processing'){
 const order={id:'dominion_'+'a'.repeat(32),account_id:'google:fixture',amount_krw:3900,status,payment_key:null}
 const state={order,grants:0,batches:0,providerCalls:[]}
 const DB={
  prepare(sql){return {bind(...values){
   const first=async()=>{
    if(sql.includes('FROM sessions s JOIN accounts'))return {id:order.account_id,credits:0}
    if(sql.startsWith('SELECT * FROM checkout_orders'))return {...order}
    if(sql.startsWith('SELECT status,payment_key FROM checkout_orders'))return {status:order.status,payment_key:order.payment_key}
    if(sql.startsWith("UPDATE checkout_orders SET status='processing'")){
     if(order.status!=='pending')return null
     order.status='processing';return {id:order.id}
    }
    throw Error('unexpected D1 first: '+sql)
   }
   return {sql,values,first,run:async()=>{throw Error('unexpected D1 run: '+sql)}}
  }}},
  async batch(statements){
   state.batches++
   assert.equal(statements.length,2)
   assert.match(statements[0].sql,/status='paid'.*status='processing'/)
   assert.match(statements[1].sql,/WHERE changes\(\)=1/)
   if(order.status==='processing'){
    order.status='paid';order.payment_key=statements[0].values[0];state.grants++
   }
   return [{success:true},{success:true}]
  }
 }
 return {state,env:env({TOSS_SECRET_KEY:'test_secret',DB})}
}
test('processing Toss order recovers via verified order lookup and grants a subscription once',async()=>{
 const {state,env:configured}=paymentFixture('processing')
 const saved=globalThis.fetch
 globalThis.fetch=async(url,options)=>{
  state.providerCalls.push({url,options})
  // Request method is inspected after successful settlement.
  return json({orderId:state.order.id,paymentKey:'pay-1',totalAmount:3900,status:'DONE'})
 }
 try{
  const recovered=await gateway.fetch(paymentRequest(state.order.id,'pay-1'),configured)
  assert.equal(recovered.status,200,JSON.stringify({body:await recovered.text(),providerCalls:state.providerCalls.map(x=>({url:x.url,method:x.options?.method}))}))
  assert.equal(state.grants,1)
  assert.equal(state.batches,1)
  assert.match(state.providerCalls[0].url,/\/v1\/payments\/orders\/dominion_/)
  const repeated=await gateway.fetch(paymentRequest(state.order.id,'pay-1'),configured)
  assert.equal(repeated.status,200)
  assert.equal(state.grants,1,'paid repeat must not extend subscription')
  const wrongKey=await gateway.fetch(paymentRequest(state.order.id,'wrong-key'),configured)
  assert.equal(wrongKey.status,409)
  assert.equal(state.grants,1)
 }finally{globalThis.fetch=saved}
})
test('uncertain Toss approve response retains processing state until successful read-only recovery',async()=>{
 const {state,env:configured}=paymentFixture('pending')
 const saved=globalThis.fetch
 let providerAttempts=0
 globalThis.fetch=async(url,options)=>{
  providerAttempts++
  if(options.method==='POST')return json({error:'temporary upstream failure'},502)
  // GET lookup may be retried, but only the first attempt may POST.
  if(providerAttempts===2)return json({error:'not settled yet'},404)
  return json({orderId:state.order.id,paymentKey:'pay-1',totalAmount:3900,status:'DONE'})
 }
 try{
  const initial=await gateway.fetch(paymentRequest(state.order.id,'pay-1'),configured)
  assert.equal(initial.status,503)
  assert.equal(state.order.status,'processing')
  assert.equal(state.grants,0)
  const stillPending=await gateway.fetch(paymentRequest(state.order.id,'pay-1'),configured)
  assert.equal(stillPending.status,503)
  assert.equal(state.order.status,'processing')
  const repaired=await gateway.fetch(paymentRequest(state.order.id,'pay-1'),configured)
  assert.equal(repaired.status,200)
  assert.equal(state.grants,1)
  assert.equal(providerAttempts,3,'do not POST the payment again')
 }finally{globalThis.fetch=saved}
})
test('Toss lookup identity mismatch cannot mint a Pro subscription',async()=>{
 const {state,env:configured}=paymentFixture('processing')
 const saved=globalThis.fetch
 globalThis.fetch=async()=>json({orderId:'different',paymentKey:'pay-1',totalAmount:3900,status:'DONE'})
 try{
  const response=await gateway.fetch(paymentRequest(state.order.id,'pay-1'),configured)
  assert.equal(response.status,503)
  assert.equal(state.grants,0)
  assert.equal(state.order.status,'processing')
 }finally{globalThis.fetch=saved}
})
