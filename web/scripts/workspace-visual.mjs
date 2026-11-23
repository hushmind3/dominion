import {withBrowser,assert} from './ui-smoke-utils.mjs'
await withBrowser('workspace-visual',async({page,db,url,viewport})=>{
 await page.goto(url+'/#/chat')
 await page.locator('.conversation-composer').waitFor({state:'visible'})
 const nav=page.locator('#workspace-navigation')
 if(!(await nav.isVisible())){
  await page.locator('.conversation-composer-options button').first().click()
  await nav.waitFor({state:'visible'})
 }
 const connections=nav.getByRole('button',{name:/서비스 연결|Connected services/})
 await connections.click()
 await page.waitForURL(/#\/connections/)
 const connectionTabs=page.getByRole('navigation',{name:/서비스 연결 보기|Connection views/})
 await connectionTabs.getByRole('button',{name:/^MCP 연결$|^MCP connections$/}).click()
 await page.waitForURL(/#\/connections\?view=mcp/)
 // Native capabilities are no longer duplicated in the MCP manager.
 assert.equal(await page.getByRole('button',{name:/도구 목록 보기|View available tools/}).count(),0)
 const plugin=page.getByRole('checkbox',{name:/MCP.*Mock Test MCP/})
 await plugin.waitFor({state:'visible'})
 assert.equal(await plugin.isChecked(),true)
 await plugin.click() // React commits the server-confirmed state asynchronously.
 await page.waitForFunction(()=>document.querySelector('input[aria-label*="Mock Test MCP"]')?.checked===false)
 assert.equal(db.mcp[0].enabled,false)
 await plugin.click()
 await page.waitForFunction(()=>document.querySelector('input[aria-label*="Mock Test MCP"]')?.checked===true)
 assert.equal(db.mcp[0].enabled,true)
 assert.equal(db.mcpWrites.length,2)
 await page.goto(url+'/#/settings?view=features')
 await page.getByRole('button',{name:/Web Search/}).waitFor({state:'visible'})
 const width=await page.evaluate(()=>document.documentElement.scrollWidth-window.innerWidth)
 assert.ok(width<12,'horizontal overflow '+width+' at '+viewport.width)
},[{width:1440,height:900},{width:820,height:820},{width:390,height:844}])
