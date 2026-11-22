import test from 'node:test'
import assert from 'node:assert/strict'
import {explicitWebSearchRequested} from '../src/utils/webSearchIntent.ts'
test('explicit search does not activate on normal chat or negative instructions',()=>{
 for(const s of ['검색해줘','웹 검색해서 확인해','search the web','인터넷에서 찾아봐'])assert.equal(explicitWebSearchRequested(s),true,s)
 for(const s of ['안녕하세요','검색하지 말고','검색 없이 알려줘','do not search the web','without browsing'])assert.equal(explicitWebSearchRequested(s),false,s)
})
