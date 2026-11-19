import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {classifyRuntimeFailure,memoryFit,modelPreflight,requiredMemory,selectRecommended,selectSmaller,planDownloadLaunch,preferredServerModelFormat} from '../src/utils/modelHardware.ts'

const gguf=(id,bytes)=>({id,format:'gguf',bytes})
const mlx=(id,bytes)=>({id,format:'mlx',bytes})
test('unknown memory does not produce invented hardware recommendations',()=>{
 assert.equal(memoryFit(4_000,0),'unknown')
 assert.deepEqual(selectRecommended([gguf('q4',4000)],0,0),{option:null,basis:null})
})
test('GPU and system RAM are independent budgets',()=>{
 const candidates=[gguf('q4',4_000),gguf('q8',8_000),mlx('mlx',9_000)]
 assert.deepEqual(selectRecommended(candidates,12_000,6_000).option?.id,'q4')
 assert.equal(selectRecommended([mlx('mlx',9_000)],0,15_000).option,null)
 assert.equal(selectRecommended([gguf('q8',8_000)],12_000,1_000).basis,'ram')
 assert.equal(requiredMemory(8_000),10_400)
})
test('fallback quantization is smaller and flagged when memory cannot fit it',()=>{
 const candidates=[gguf('q2',2_000),gguf('q4',4_000),gguf('q8',8_000)]
 assert.deepEqual(selectSmaller(candidates,8_000,6_000,0),{option:candidates[1],fits:true})
 assert.deepEqual(selectSmaller(candidates,8_000,1_000,0),{option:candidates[0],fits:false})
 assert.equal(selectSmaller(candidates,2_000,8_000,0).option,null)
})
test('model launch failures distinguish memory, files and engines',()=>{
 assert.equal(classifyRuntimeFailure('CUDA out of memory'),'memory')
 assert.equal(classifyRuntimeFailure('MLX 모델 파일이 완전하지 않습니다.'),'file')
 assert.equal(classifyRuntimeFailure('llama-server를 찾을 수 없습니다.'),'engine')
 assert.equal(classifyRuntimeFailure('MLX 실행기가 설치되지 않았습니다.'),'engine')
 assert.equal(classifyRuntimeFailure('unexpected exit'),'other')
})
test('model menu renders only one Hugging Face browser and unique tab id',()=>{
 const source=readFileSync(new URL('../src/pages/Models.tsx',import.meta.url),'utf8')
 assert.equal([...source.matchAll(/id:'huggingface'/g)].length,1)
 assert.equal([...source.matchAll(/<ModelExplorer[^>]*initialView="search"/g)].length,1)
 assert.match(source,/hideManagementNavigation/)
})

test('queued model launch resumes interrupted downloads instead of leaving them paused',()=>{
 assert.equal(planDownloadLaunch(undefined,false),'start')
 assert.equal(planDownloadLaunch('downloading',false),'wait')
 assert.equal(planDownloadLaunch('resolving',false),'wait')
 assert.equal(planDownloadLaunch('paused',false),'resume')
 assert.equal(planDownloadLaunch('failed',false),'resume')
 assert.equal(planDownloadLaunch('cancelled',false),'resume')
 assert.equal(planDownloadLaunch('completed',false),'start')
 assert.equal(planDownloadLaunch('completed',true),'installed')
})

test('local model format follows the actual Rust server, not the browser OS',()=>{
 const macMlx={available:true,installable:false}
 const macNeedsSetup={available:false,installable:true}
 const unavailable={available:false,installable:false}
 assert.equal(preferredServerModelFormat(undefined,undefined,macMlx,{available:false}),null)
 assert.equal(preferredServerModelFormat('win32','x86_64',macMlx,{available:false}),'gguf')
 assert.equal(preferredServerModelFormat('linux','aarch64',macMlx,{available:false}),'gguf')
 assert.equal(preferredServerModelFormat('macos','x86_64',macMlx,{available:false}),'gguf')
 assert.equal(preferredServerModelFormat('macos','aarch64',macMlx,{available:false}),'mlx')
 assert.equal(preferredServerModelFormat('macos','arm64',macNeedsSetup,{available:false}),'mlx')
 assert.equal(preferredServerModelFormat('macos','aarch64',unavailable,{available:false}),'gguf')
 assert.equal(preferredServerModelFormat('macos','aarch64',macNeedsSetup,{available:true}),'gguf')
 assert.equal(preferredServerModelFormat('macos','aarch64',macMlx,{available:true}),'mlx')
})
test('model format selection uses runtime capabilities and preserves manual choices',()=>{
 const source=readFileSync(new URL('../src/components/ModelExplorer.tsx',import.meta.url),'utf8')
 assert.doesNotMatch(source,/navigator\.platform/)
 assert.match(source,/preferredServerModelFormat\(runtime\.capabilities\?\.memory\.platform/)
 assert.match(source,/if\(preferredFormat&&!userChoseFormat\.current\)setFormat\(preferredFormat\)/)
 assert.match(source,/userChoseFormat\.current=true;setFormat\(item\.id\)/)
})

test('preflight blocks missing native GGUF engine even when LM Studio is available',()=>{
 const caps={gguf:{available:true,path:null},mlx:{available:true,standalone:false},lmstudio:{available:true},memory:{free:8_000},gpu:{free:0}}
 assert.deepEqual(modelPreflight(gguf('managed-model.gguf',4_000),caps).status,'blocked')
 assert.equal(modelPreflight({id:'lms:imported',format:'gguf',bytes:4_000},caps).status,'ready')
 assert.equal(modelPreflight({id:'mlx:owner/model',format:'mlx',bytes:4_000},caps).status,'ready')
 assert.equal(modelPreflight({id:'external:mlx',format:'mlx',bytes:4_000},caps).status,'blocked')
})
test('preflight uses separate memory budgets and warns rather than forbidding a risky launch',()=>{
 const caps={gguf:{available:true,path:'/bin/llama-server'},mlx:{available:true,standalone:true},lmstudio:{available:false},memory:{free:2_000},gpu:{free:8_000}}
 assert.equal(modelPreflight(gguf('model',4_000),caps).status,'ready')
 assert.equal(modelPreflight(mlx('model',4_000),caps).status,'warning')
 assert.equal(modelPreflight(gguf('large',10_000),caps).status,'warning')
 assert.equal(modelPreflight(gguf('model',4_000),undefined).status,'unknown')
 assert.equal(modelPreflight({id:'lms:test',format:'gguf',bytes:4_000},caps).status,'blocked')
})
test('both installed and Hugging Face launch use the same fresh preflight',()=>{
 const source=readFileSync(new URL('../src/components/ModelExplorer.tsx',import.meta.url),'utf8')
 assert.match(source,/const snapshot=await get<Runtime>\('\/api\/model-library\/runtime'\)/)
 assert.match(source,/const check=modelPreflight\(model,snapshot.capabilities\)/)
 assert.match(source,/if\(check.status==='blocked'\)/)
 assert.match(source,/selectedPreflight&&<div/)
})
