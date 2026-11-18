import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {classifyRuntimeFailure,memoryFit,requiredMemory,selectRecommended,selectSmaller} from '../src/utils/modelHardware.ts'

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
