import {withBrowser,assert} from './ui-smoke-utils.mjs'
const png=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9UFZhcsAAAAASUVORK5CYII=','base64')
await withBrowser('chat-attachments-visual',async({page,db,url})=>{
 await page.goto(url+'/#/chat')
 const form=page.locator('.conversation-composer')
 await form.waitFor({state:'visible'})
 await page.locator('input[type="file"]').setInputFiles({name:'fixture.png',mimeType:'image/png',buffer:png})
 const preview=page.locator('img[alt="fixture.png"]')
 await preview.waitFor({state:'visible'})
 await page.getByRole('button',{name:/첨부.*삭제|첨부.*제거|Remove.*attachment|이미지.*삭제/}).first().click()
 assert.equal(await preview.count(),0,'removing preview must free pending attachment')
 assert.equal(db.chatRequests.length,0,'attachment selection alone must not send a model request')
},[{width:1280,height:900}])
