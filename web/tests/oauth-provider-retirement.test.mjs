import assert from 'node:assert/strict'
import {test} from 'node:test'
import {isOAuthAccountId, supportedOAuthAccounts} from '../src/utils/oauthAccounts.ts'

test('OAuth provider catalog excludes retired upload integrations',()=>{
 const entries=[
  {id:'youtube',name:'Old upload grant',configured:true,connected:true},
  {id:'github',name:'GitHub',configured:true,connected:true},
  {id:'google',name:'Google',configured:true,connected:false},
  {id:'unknown',name:'Unregistered account',configured:true,connected:true}
 ]
 assert.deepEqual(supportedOAuthAccounts(entries).map(row=>row.id),['github','google'])
 assert.equal(isOAuthAccountId('youtube'),false)
 assert.equal(isOAuthAccountId('github'),true)
 assert.equal(isOAuthAccountId('google'),true)
 assert.deepEqual(supportedOAuthAccounts([]),[])
})
