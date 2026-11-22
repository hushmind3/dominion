import {test} from 'node:test'
import assert from 'node:assert/strict'
import {decodeNativeGoogleFlight,encodeNativeGoogleFlight,nativeGoogleFlightTtlMs} from '../src/utils/cloudAuthFlight.ts'

test('native Google OAuth flow stays recoverable across navigation until expiry',()=>{
 const id='A'.repeat(43)
 const now=Date.UTC(2026,10,19,0,0,0)
 const encoded=encodeNativeGoogleFlight(id,now)
 assert.equal(decodeNativeGoogleFlight(encoded,now+1),id)
 assert.equal(decodeNativeGoogleFlight(encoded,now+nativeGoogleFlightTtlMs), '')
})

test('expired, corrupted and forged state identifiers are never restored',()=>{
 for(const payload of [null,'not-json','{}',JSON.stringify({id:'bad',expiresAt:Date.now()+1000000}),JSON.stringify({id:'B'.repeat(43),expiresAt:0})]){
  assert.equal(decodeNativeGoogleFlight(payload),'')
 }
})
