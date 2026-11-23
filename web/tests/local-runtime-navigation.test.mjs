import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {localRuntimeSetupHref,modelHubLanding} from '../src/utils/modelHubRoute.ts'

const source=(path)=>readFileSync(new URL(path,import.meta.url),'utf8')

test('chat local setup navigates to existing runtime installer view',()=>{
 assert.equal(localRuntimeSetupHref,'#/models?tab=local&view=import')
 assert.deepEqual(modelHubLanding(localRuntimeSetupHref),{tab:'local',localView:'import'})
 assert.deepEqual(modelHubLanding('#/models?tab=local&view=search'),{tab:'local',localView:'search'})
 assert.deepEqual(modelHubLanding('#/models?tab=local&view=removed'),{tab:'local',localView:'installed'})
 assert.deepEqual(modelHubLanding('#/chat?tab=local&view=import'),{tab:'providers',localView:'installed'})
})

test('model hub reuses one existing runtime installer, legacy local gateways stay advanced',()=>{
 const hub=source('../src/pages/Models.tsx')
 const explorer=source('../src/components/ModelExplorer.tsx')
 assert.equal([...hub.matchAll(/<ModelExplorer initialView=/g)].length,1)
 assert.match(hub,/modelHubLanding\(window\.location\.hash\)/)
 assert.match(hub,/외부 로컬 서버 연결 \(고급\)/)
 assert.match(hub,/tab==='local'&&<div className="mb-4"><ModelExplorer/)
 const runtime=explorer.indexOf("t('models.runtimes')")
 const importFile=explorer.indexOf("t('models.importTitle')")
 assert.ok(runtime>=0&&importFile>runtime,'install engines before asking for file path')
 assert.match(explorer,/\/api\/model-library\/runtime\/install/)
 assert.match(explorer,/\/api\/model-library\/runtimes/)
})

test('chat handles missing engine by preserving model and opening same setup instead of duplicating installer',()=>{
 const chat=source('../src/pages/Chat.tsx')
 const picker=source('../src/components/chat/ConversationTargetPicker.tsx')
 assert.match(chat,/modelPreflight\(\{id:m\.id,format:m\.format,bytes:m\.bytes\},state\.capabilities\)/)
 assert.match(chat,/openLocalRuntimeSetup\(m\)/)
 assert.match(chat,/dominion-model-engine-retry/)
 assert.match(chat,/window\.location\.hash=localRuntimeSetupHref/)
 assert.ok(picker.includes('onLocalSetup:()=>void'))
 assert.match(picker,/실행기 설치 · 로컬 모델 관리/)
})
