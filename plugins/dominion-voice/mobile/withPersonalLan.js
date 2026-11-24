const {withAndroidManifest,withDangerousMod}=require('expo/config-plugins');
const fs=require('fs');
const path=require('path');
module.exports=config=>{
 config=withAndroidManifest(config,c=>{
  const app=c.modResults.manifest.application[0];
  app.$['android:usesCleartextTraffic']='true';
  app.$['android:networkSecurityConfig']='@xml/dominion_personal_network';
  return c;
 });
 return withDangerousMod(config,['android',async c=>{
  const root=path.join(c.modRequest.platformProjectRoot,'app/src/main/res/xml');
  fs.mkdirSync(root,{recursive:true});
  fs.writeFileSync(path.join(root,'dominion_personal_network.xml'),'<?xml version="1.0" encoding="utf-8"?><network-security-config><base-config cleartextTrafficPermitted="true"/></network-security-config>');
  return c;
 }]);
};
