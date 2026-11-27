// CI packaging uses a source folder, so Finder/Spotlight cannot hold a mounted DMG.
import {spawnSync} from 'node:child_process';
import {cpSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync} from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export function packageDmg(appPath, outputPath, volumeName) {
  if (process.platform !== 'darwin') throw Error('DMG packaging requires macOS');
  if (!existsSync(appPath) || !appPath.endsWith('.app')) throw Error('A built .app bundle is required');
  const stage=mkdtempSync(path.join(os.tmpdir(),'dominion-dmg-'));
  function run(args) {
    const result=spawnSync('hdiutil',args,{stdio:'inherit',shell:false});
    if(result.error)throw result.error;
    if(result.status!==0)throw Error(`hdiutil ${args[0]} failed (${result.status})`);
  }
  try {
    cpSync(appPath,path.join(stage,path.basename(appPath)),{recursive:true,verbatimSymlinks:true});
    symlinkSync('/Applications',path.join(stage,'Applications'));
    mkdirSync(path.dirname(outputPath),{recursive:true});
    run(['create','-volname',volumeName,'-srcfolder',stage,'-ov','-format','UDZO',outputPath]);
    run(['verify',outputPath]);
  } finally {
    rmSync(stage,{recursive:true,force:true});
  }
}
