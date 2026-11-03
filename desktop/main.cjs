'use strict';
const {app,BrowserWindow,shell,dialog,session,ipcMain} = require('electron');
const {spawn} = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const net = require('node:net');
const ROOT=path.resolve(__dirname,'..');
let window=null,engine=null,quitting=false,enginePort=0;
if(!app.requestSingleInstanceLock()){app.quit();}
else{
 app.on('second-instance',()=>{window?.restore();window?.focus();});
 app.whenReady().then(start).catch(err=>{dialog.showErrorBox('DOMINION 시작 실패',String(err));app.quit()});
}
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function freePort(){return new Promise((resolve,reject)=>{const s=net.createServer();s.once('error',reject);s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>resolve(p))})})}
async function start(){
 ipcMain.removeHandler('dominion:pick-model');
 ipcMain.handle('dominion:pick-model',async(_event,format)=>{
  const folder=format==='mlx';
  const answer=await dialog.showOpenDialog(window||undefined,{
   title:folder?'MLX 모델 폴더 선택':'GGUF 모델 파일 선택',
   properties:folder?['openDirectory']:['openFile'],
   ...(folder?{}:{filters:[{name:'GGUF models',extensions:['gguf']} ]})
  });
  return answer.canceled?'':answer.filePaths[0]||'';
 });
 if(!fs.existsSync(path.join(ROOT,'dist','index.html')))throw Error('Missing compiled UI. Run npm run build in the repository root.');
 enginePort=await freePort();
 const data=path.join(app.getPath('userData'),'runtime');
 const workspace=path.join(app.getPath('documents'),'DOMINION Workspace');
 fs.mkdirSync(data,{recursive:true});fs.mkdirSync(workspace,{recursive:true});
 engine=spawn(process.execPath,[path.join(ROOT,'server/index.mjs')],{
  cwd:ROOT,stdio:['ignore','pipe','pipe'],windowsHide:true,
  env:{...process.env,ELECTRON_RUN_AS_NODE:'1',DOMINION_PORT:String(enginePort),DOMINION_HOST:'127.0.0.1',DOMINION_DATA:data,DOMINION_WORKSPACE:workspace}
 });
 let stderr='';
 engine.stderr.on('data',d=>{stderr=(stderr+d.toString()).slice(-6000);});
 engine.on('exit',(code,signal)=>{if(!quitting){dialog.showErrorBox('DOMINION Gateway 종료','Engine exited unexpectedly: '+code+' '+signal+'\n'+stderr);app.quit();}});
 const origin='http://127.0.0.1:'+enginePort;
 let ready=false;
 for(let i=0;i<100;i++){
  if(engine.exitCode!==null)throw Error('Gateway exited: '+stderr);
  try{const r=await fetch(origin+'/api/health',{signal:AbortSignal.timeout(500)});if(r.ok){ready=true;break}}catch{}
  await sleep(160);
 }
 if(!ready)throw Error('DOMINION gateway startup timed out: '+stderr);
 window=new BrowserWindow({title:'DOMINION',width:1460,height:940,minWidth:780,minHeight:620,backgroundColor:'#f6f8fc',show:false,
   webPreferences:{nodeIntegration:false,contextIsolation:true,sandbox:true,webSecurity:true,preload:path.join(__dirname,'preload.cjs')}
 });
 session.defaultSession.setPermissionRequestHandler((_webContents,permission,callback)=>{
  callback(permission==='clipboard-sanitized-write');
 });
 window.webContents.setWindowOpenHandler(({url})=>{
  try{
   const u=new URL(url);
   const oauth=(u.hostname==='github.com'&&u.pathname==='/login/oauth/authorize')||
       (u.hostname==='accounts.google.com'&&u.pathname.startsWith('/o/oauth2/'));
   if(oauth&&u.protocol==='https:')return {action:'allow',overrideBrowserWindowOptions:{webPreferences:{nodeIntegration:false,contextIsolation:true,sandbox:true}}};
   if(u.protocol==='https:')void shell.openExternal(url);
  }catch{}
  return {action:'deny'};
 });
 window.webContents.on('will-navigate',(e,url)=>{
  if(!url.startsWith(origin+'/')){
   e.preventDefault();
   try{if(new URL(url).protocol==='https:')void shell.openExternal(url)}catch{}
  }
 });
 window.once('ready-to-show',()=>window.show());
 await window.loadURL(origin);
 app.on('activate',()=>{if(BrowserWindow.getAllWindows().length===0)void start();});
}
app.on('window-all-closed',()=>{if(process.platform!=='darwin')app.quit();});
app.on('before-quit',()=>{quitting=true;if(engine&&!engine.killed)engine.kill();});
