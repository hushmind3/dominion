#!/usr/bin/env node
// Render the actual video workspace with the shipping React bundle.
// No browser, model, GPU, credentials or paid API calls are required.
import assert from 'node:assert/strict'
import {mkdtemp,rm,readFile} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import {dirname,join,resolve} from 'node:path'
import {fileURLToPath} from 'node:url'
import {createRequire} from 'node:module'
import {build} from 'esbuild'
import React from 'react'
import {renderToStaticMarkup} from 'react-dom/server'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..')
const staging=await mkdtemp(join(root,'.studio-smoke-'))
const built=join(staging,'studio.cjs')
try {
 await build({
  entryPoints:[join(root,'src/pages/Video.tsx')],
  bundle:true,platform:'node',format:'cjs',jsx:'automatic',target:'node24',
  external:['react','react-dom','react/jsx-runtime'],outfile:built,logLevel:'error',
 })
 const {VideoPage}=createRequire(import.meta.url)(built)
 const html=renderToStaticMarkup(React.createElement(VideoPage,{navigate:()=>{}}))
 for(const text of ['DOMINION VIDEO STUDIO','아이디어에서 완성 영상까지',
  '자율 콘텐츠 제작','수동 제작','작업함','YouTube 게시','환경설정',
  'CONTENT AUTOPILOT','새 콘텐츠 제작','장면별 대본','절약 모드','자율 콘텐츠 제작 시작',
  'ComfyUI','미리보기 · 작업 현황']) {
  assert.ok(html.includes(text),'Missing studio entry point: '+text)
 }
 assert.match(html,/제작 프로젝트/)
 assert.match(html,/type="checkbox"/)
 assert.match(html,/기획·대본|장면 구성/)
 assert.match(html,/aria-current="page"/)
 assert.ok(!html.includes('undefined%'),'No fabricated job progress')
 assert.ok(html.includes('CONTENT AUTOPILOT'),'Production workflow must be visible by default')
 const code=await readFile(join(root,'src/pages/Video.tsx'),'utf8')
 for(const contract of ['tab===\x27jobs\x27','tab===\x27publish\x27',
  'tab===\x27settings\x27','/api/video/jobs','/api/video/outputs',
  '/api/video/models/downloads','/api/youtube/status',
  'onClick={()=>void startDownload()}','setSelectedJobId(job.id)']) {
  assert.ok(code.includes(contract),'Lost video operation: '+contract)
 }
 console.log(JSON.stringify({ok:true,renderedCharacters:html.length,
  workflows:['create','jobs','publish','settings'],
  evidence:'real React SSR + product route and control assertions'},null,2))
} finally {
 await rm(staging,{recursive:true,force:true})
}
