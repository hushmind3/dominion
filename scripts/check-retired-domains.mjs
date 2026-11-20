// Static scope gate: retired domain-specific products cannot quietly return.
// Search only active source, not historical evidence, documentation or guard tests.
import {existsSync,readFileSync,readdirSync} from 'node:fs'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const file=p=>path.join(root,p)
const read=p=>readFileSync(file(p),'utf8')
const bannedFiles=[
 'src/api/video.rs','src/api/rsi.rs',
 'src/engine/video.rs','src/engine/rsi.rs','src/engine/rsi_evolve.rs',
 'src/engine/rsi_worktree.rs','src/engine/money.rs',
 'web/src/pages/Video.tsx','web/src/pages/SelfImprovement.tsx',
 'web/src/pages/Money.tsx','web/src/components/ImprovementNotice.tsx',
]
for(const name of bannedFiles)if(existsSync(file(name)))throw Error('Retired domain file returned: '+name)
const api=read('src/api/mod.rs')
if(/(?:pub )?mod (?:video|rsi|money);/.test(api))throw Error('Retired module is registered')
if(/\.route\(["']\/api\/(?:video|youtube|rsi|money)(?:\/|["'])/.test(api))
 throw Error('Retired domain API remains registered')
const compat=read('src/api/compat.rs')
if(/youtube_(?:uploads|status)|api::video/.test(compat))throw Error('Media compatibility API returned')
const jobs=read('src/engine/jobs.rs')
if(/\b(?:rsi_evolve|rsi_compare|rsi_generation|youtube_upload)\b|\b"video"\s*\|/.test(jobs))
 throw Error('Retired job execution branch returned')
const algorithms=read('src/engine/algorithms.rs')
if(/ARCHIVED_KINDS|selfimprove\.search|adaptation\.selection|targetExpression/.test(algorithms))
 throw Error('Retired algorithm domain is still registered')
const nativeOAuth=read('src/engine/oauth.rs')
if(/id:\s*"google"|DOMINION_GOOGLE|accounts\.google\.com|googleapis\.com\/oauth/.test(nativeOAuth))
 throw Error('Retired native Google OAuth provider returned')
const connections=read('web/src/pages/Connections.tsx')
if(/section==='accounts'|oauthEditor|conn\.dlg\.googleTitle/.test(connections))
 throw Error('Duplicated legacy OAuth settings panel returned')
const quickAccounts=read('web/src/components/ConnectedAccounts.tsx')
if(/googleRequiresApp|googleDescription|Globe2|id==='google'/.test(quickAccounts))
 throw Error('Legacy Google/YouTube account card returned')
const operations=read('web/src/pages/Operations.tsx')
if(/ops\.accounts|oauthProvider|openOAuth\(/.test(operations))
 throw Error('Duplicate OAuth control in advanced settings')
// Old UI modules must not poll retained media/RSI/money endpoints.
for(const source of [
 'web/src/pages/Connections.tsx','web/src/pages/Operations.tsx',
 'web/src/pages/memory/Growth.tsx','web/src/pages/memory/LearningRecords.tsx'
]){
 const current=read(source)
 if(/\/api\/(?:video|youtube|rsi|money|optimizers|experiment-requests)(?:\/|['"]|$)/.test(current))
  throw Error('Retired feature API still used in frontend: '+source)
}
const errorMessages=read('web/src/i18n/errors.ts')
if(/영상 작업|video job|ComfyUI HTTP/.test(errorMessages))
 throw Error('Obsolete video error text still bundled')
const bootstrap=read('src/bootstrap.rs')
if(/--rsi-|rsiSourceDir/.test(bootstrap))throw Error('Retired CLI remained')
const strings=read('web/src/i18n/errors.ts')
if(/ComfyUI HTTP|영상 작업 만들기|Could not open the video/.test(strings))
 throw Error('Retired video UI message remains')
const pages=path.join(root,'web/src/pages')
for(const name of readdirSync(pages))if(/^(Video|Money|SelfImprovement)\.(tsx?|jsx?)$/.test(name))
 throw Error('Retired product page remains: '+name)
console.log('Retired video / money / RSI runtime surfaces absent')
