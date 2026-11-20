import {test} from 'node:test'
import assert from 'node:assert/strict'
import {readdirSync,readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import path from 'node:path'

const root=fileURLToPath(new URL('../src/',import.meta.url))
function sources(directory){
 return readdirSync(directory,{withFileTypes:true}).flatMap(entry=>{
  const full=path.join(directory,entry.name)
  if(entry.isDirectory())return sources(full)
  return /\.(?:ts|tsx|css)$/.test(entry.name)?[full]:[]
 })
}

test('retired video producer, YouTube sign-in and independent improvement UI do not re-enter frontend',()=>{
 const forbidden=[
  /(?:YouTube|youtu\.be|유튜브)/i,
  /(?:ComfyUI|ffmpeg|video[_-]studio|video[_-]autopilot)/i,
  /(?:rsiRun|rsi[_-]campaign|selfimprove\.search)/i,
  /(?:영상\s*제작|비디오\s*제작|머니\s*탐색)/,
  /(?:rooms\.selfReview|jobk\.video)/,
  /\/api\/(?:improvements|experiment-requests|policy-searches|optimizers|algorithms\/evaluate(?:ions)?)(?:\b|\/)/
 ]
 for(const file of sources(root)){
  const source=readFileSync(file,'utf8')
  for(const pattern of forbidden)
   assert.doesNotMatch(source,pattern,'obsolete UI in '+path.relative(root,file))
 }
})
