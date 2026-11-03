'use strict';
const {contextBridge,ipcRenderer}=require('electron');
contextBridge.exposeInMainWorld('dominionDesktop',Object.freeze({
 pickModel:(format)=>ipcRenderer.invoke('dominion:pick-model',format==='mlx'?'mlx':'gguf'),
 pickVideo:()=>ipcRenderer.invoke('dominion:pick-video'),
 showItem:(target)=>ipcRenderer.invoke('dominion:show-item',target),
 openPath:(target)=>ipcRenderer.invoke('dominion:open-path',target)
}));
