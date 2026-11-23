// Optional live read-only API probe. Fast CI syntax-checks this file;
// manually run with DOMINION_URL to validate an actual serving backend.
import assert from 'node:assert/strict'
const url=process.env.DOMINION_URL
if(url){
 const res=await fetch(new URL('/api/health',url),{signal:AbortSignal.timeout(8000)})
 assert.equal(res.ok,true,'backend should answer /api/health')
 const body=await res.json()
 assert.ok(body&&typeof body==='object','backend health response must be structured')
 console.log('Live DOMINION health verified')
}else{
 console.log('Provide DOMINION_URL to run the live API integration probe.')
}
