import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,statSync,rmSync} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {packageDmg} from './package-dmg.mjs';

test('CI packages and verifies a disk image from an app folder with spaces', {skip:process.platform!=='darwin'},()=>{
  const root=mkdtempSync(path.join(os.tmpdir(),'dominion-dmg-test-'));
  try{
    const app=path.join(root,'DOMINION Test.app');
    mkdirSync(path.join(app,'Contents'),{recursive:true});
    writeFileSync(path.join(app,'Contents','fixture.txt'),'native packaging regression');
    const image=path.join(root,'output with spaces','test.dmg');
    packageDmg(app,image,'DOMINION Test');
    assert.ok(statSync(image).size>0);
  }finally{rmSync(root,{recursive:true,force:true});}
});
