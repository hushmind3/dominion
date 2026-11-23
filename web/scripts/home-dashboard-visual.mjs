import {withBrowser,assert} from './ui-smoke-utils.mjs'
await withBrowser('home-dashboard-visual',async({page,db,url})=>{
 await page.goto(url+'/#/home')
 const recent=page.locator('.workspace-page-content').getByRole('button',{name:/Recent Test Chat/})
 await recent.waitFor({state:'visible'})
 assert.equal(db.sessions[0].title,'Recent Test Chat')
 await recent.click()
 await page.waitForURL(/#\/chat/)
 await page.locator('.conversation-composer').waitFor({state:'visible'})
 assert.ok(page.url().includes('recent-chat'),'recent conversation navigation must retain identity')
},[{width:1280,height:900}])
