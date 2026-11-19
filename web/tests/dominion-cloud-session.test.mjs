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
