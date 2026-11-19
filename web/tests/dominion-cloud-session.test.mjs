import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'

const cloud=readFileSync(new URL('../src/components/DominionCloudAccount.tsx',import.meta.url),'utf8')
test('existing cloud model re-registration refreshes the vault credential instead of duplicating',()=>{
 const section=cloud.slice(cloud.indexOf('async function register()'),cloud.indexOf('const filteredCatalog=',cloud.indexOf('async function register()')))
 assert.match(section,/if\(match\)\{/)
 assert.match(section,/post<\{id:string\}>\('\/api\/providers',\{\.\.\.match,apiKey:token,enabled:true\}\)/)
 assert.match(section,/setRegistered\(\{providerId:renewed\.id/)
})
test('UI logout attempts server-side session revocation',()=>{
 const section=cloud.slice(cloud.indexOf('function logout()'),cloud.indexOf('async function loadCatalog()',cloud.indexOf('function logout()')))
 assert.match(section,/sessionStorage\.removeItem\('dominion_cloud_session'\)/)
 assert.match(section,/fetch\(cloudUrl\+'\/v1\/auth\/logout'/)
 assert.match(section,/method:'POST'/)
})
test('cloud registration uses the same native Rust provider route',()=>{
 assert.match(cloud,/post<\{id:string\}>\('\/api\/providers',\{name:'DOMINION Cloud/)
 assert.match(cloud,/baseUrl:cloudUrl\+'\/v1'/)
 assert.match(cloud,/toolMode:'compat'/)
})

test('Google sign-in retains issued session even when catalog or billing reads fail',()=>{
 const start=cloud.indexOf('window.google.accounts.id.initialize(')
 const end=cloud.indexOf('window.google.accounts.id.renderButton(',start)
 const callback=cloud.slice(start,end)
 const persisted=callback.indexOf("sessionStorage.setItem('dominion_cloud_session',data.token)")
 const fetched=callback.indexOf('Promise.allSettled([fetchModels(data.token),loadBilling(data.token)])')
 assert.ok(persisted>=0&&fetched>persisted,'save the authenticated session before dependent reads')
})
test('restored Cloud session is cleared only after explicit 401 and not on transient outage',()=>{
 const block=cloud.slice(cloud.indexOf("const pendingKey='dominion_cloud_pending_payment'"),cloud.indexOf('async function fetchModels(session:string)'))
 assert.match(block,/Promise\.allSettled\(\[fetchModels\(saved\),loadBilling\(saved\)\]\)/)
 assert.match(block,/session\.status===401/)
 assert.ok(block.indexOf('session.status===401')<block.lastIndexOf("sessionStorage.removeItem('dominion_cloud_session')"))
})
test('Toss redirect is durable across refresh and is cleared only on verified paid status',()=>{
 const section=cloud.slice(cloud.indexOf("const pendingKey='dominion_cloud_pending_payment'"),cloud.indexOf('async function fetchModels(session:string)'))
 assert.match(section,/sessionStorage\.setItem\(pendingKey,JSON\.stringify\(\{orderId,paymentKey,amount\}\)\)/)
 assert.match(section,/sessionStorage\.getItem\(pendingKey\)/)
 assert.match(section,/data\.status!=='paid'/)
 assert.match(section,/sessionStorage\.removeItem\(pendingKey\)/)
 assert.match(section,/새로고침하면 같은 주문을 다시 조회합니다/)
})
