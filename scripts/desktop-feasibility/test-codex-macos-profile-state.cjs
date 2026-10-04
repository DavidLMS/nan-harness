'use strict';
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert/strict');
if(process.platform!=='darwin'){console.log('SKIP native macOS descriptor transport');process.exit(0);}
const {authority}=require('./codex-profile-state.cjs');
const root=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'codex-state-fixture-')));
try {
  fs.chmodSync(root,0o700);
  const suffixes=['','profile','profile/home','profile/config','profile/nanh','profile/nanh/chatgpt-desktop','profile/nanh/chatgpt-desktop/profile','profile/codex-desktop'];
  const directories=suffixes.map(suffix=>{
    const directory=path.join(root,suffix);if(suffix)fs.mkdirSync(directory,{mode:0o700});
    const m=fs.statSync(directory,{bigint:true});
    return {path:directory,device:String(m.dev),inode:String(m.ino),uid:Number(m.uid),mode:448};
  });
  const loan={schemaVersion:1,platform:'macos',directories,stateRootIndex:6,stateBasename:'.codex-global-state.json',diagnosticsOnly:true};
  const state=path.join(directories[6].path,loan.stateBasename);
  fs.writeFileSync(state,JSON.stringify({synthetic:true}),{mode:0o600});
  const held=authority(loan,Date.now()+5000);
  assert(held);
  try {
    const pair=held.snapshotPair();assert(pair);assert.equal(pair.first.value.synthetic,true);
    fs.chmodSync(state,0o644);assert.equal(held.snapshotPair(),null);
    fs.chmodSync(state,0o600);
    fs.renameSync(directories[6].path,directories[6].path+'.old');
    fs.mkdirSync(directories[6].path,{mode:0o700});
    assert.equal(held.verify(),false);assert.equal(held.snapshotPair(),null);
  } finally {held.close();}
  assert.equal(authority({...loan,platform:'linux'},Date.now()+5000),null);
  console.log('PASS native fixed-state pair rejects nonprivate state and replaced original root');
} finally {fs.rmSync(root,{recursive:true,force:true});}
