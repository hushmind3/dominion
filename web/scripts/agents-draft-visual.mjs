import {withBrowser,assert} from './ui-smoke-utils.mjs'
await withBrowser('agents-draft-visual',async({page,db,url})=>{
 await page.goto(url+'/#/prompts?view=personas')
 await page.waitForURL(/#\/memory\?view=personas/)
 const tabs=page.getByRole('navigation',{name:/기억과 지침|Memory and instructions/})
 await tabs.waitFor({state:'visible'})
 const personas=tabs.getByRole('button',{name:/AI·페르소나|AIs & personas/})
 assert.equal(await personas.getAttribute('aria-current'),'page')
 await page.getByText('Test Persona').first().waitFor({state:'visible'})
 assert.equal(db.agents[0].id,'test-persona')
},[{width:1280,height:850},{width:390,height:844}])
