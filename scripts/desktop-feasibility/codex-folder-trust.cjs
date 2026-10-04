'use strict';
const fs=require('node:fs');
const path=require('node:path');
function authority(ownedWorkspace, io=fs, platform=process.platform) {
  let fd;
  const paths=platform==='win32'?path.win32:path.posix;
  const invalid={workspace:'',verify:()=>false,close:()=>{}};
  try {
    if(typeof ownedWorkspace!=='string'||ownedWorkspace.includes('\0'))return invalid;
    const workspace=platform==='win32'&&/^\\\\\?\\[A-Za-z]:\\/.test(ownedWorkspace)
      ?ownedWorkspace.slice(4):ownedWorkspace;
    if(!paths.isAbsolute(workspace)||platform==='win32'&&!/^[A-Za-z]:\\/.test(workspace))return invalid;
    const ancestors=[];
    for(let current=workspace;;current=paths.dirname(current)) {
      const stat=io.lstatSync(current,{bigint:true});
      if(!stat.isDirectory()||stat.isSymbolicLink()||stat.ino<=0n)return invalid;
      ancestors.push({path:current,dev:stat.dev,ino:stat.ino,birth:stat.birthtimeNs});
      if(paths.dirname(current)===current)break;
    }
    const canonical=io.realpathSync.native(workspace);
    const normal=value=>platform==='win32'&&/^\\\\\?\\[A-Za-z]:\\/.test(value)?value.slice(4):value;
    if(normal(canonical)!==workspace)return invalid;
    const own=io.lstatSync(workspace,{bigint:true});
    if(platform!=='win32'&&(own.uid!==BigInt(process.getuid())||(own.mode&0o077n)!==0n))return invalid;
    try {fd=io.openSync(workspace,fs.constants.O_RDONLY|(fs.constants.O_NOFOLLOW||0));}
    catch(error){if(platform!=='win32'||!['EPERM','EACCES','EISDIR'].includes(error.code))return invalid;}
    return {workspace,verify:()=>{
      try {
        if(normal(io.realpathSync.native(workspace))!==workspace)return false;
        for(const held of ancestors){const current=io.lstatSync(held.path,{bigint:true});
          if(!current.isDirectory()||current.isSymbolicLink()||current.dev!==held.dev
            ||current.ino!==held.ino||current.birthtimeNs!==held.birth)return false;}
        if(fd!==undefined){const current=io.fstatSync(fd,{bigint:true});
          if(current.dev!==own.dev||current.ino!==own.ino)return false;}
        return true;
      } catch{return false;}
    },close:()=>{if(fd!==undefined){io.closeSync(fd);fd=undefined;}}};
  } catch {if(fd!==undefined)io.closeSync(fd);return invalid;}
}

// Pinned project-folder-consent-dialog components: Linux 5ab3b26d..., Mac
// 24a177c6..., Windows 3fbae19f.... Authority is private caller-owned state.
function sample({workspace,held}) {
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden';};
  const dialogs=[...document.querySelectorAll('[role="dialog"],[role="alertdialog"],[aria-modal="true"],[role="menu"]')].filter(visible);
  if(dialogs.length===0)return {status:'absent'};
  if(dialogs.length!==1||dialogs[0].getAttribute('role')!=='dialog')return {status:'blocked',rejectionStage:'dialog'};
  const dialog=dialogs[0];
  const forms=[...dialog.querySelectorAll('form.select-none')].filter(visible);
  if(forms.length!==1)return {status:'blocked',rejectionStage:'form'};
  const form=forms[0];
  const titles=[...form.querySelectorAll('h2.contents')].filter(e=>{
    const style=getComputedStyle(e);
    return e.isConnected&&style.display==='contents'&&style.visibility!=='hidden'
      &&e.textContent.trim()==='Trust this folder?';
  });
  const lists=[...form.querySelectorAll('ul.flex.flex-col.select-text')].filter(visible);
  const items=lists.length===1?[...lists[0].children]:[];
  const buttons=[...form.querySelectorAll('button')].filter(visible);
  const trust=buttons.filter(e=>e.textContent.trim()==='Trust folder'&&e.getAttribute('type')==='submit');
  const cancel=buttons.filter(e=>e.textContent.trim()==='Cancel'&&e.getAttribute('type')==='button');
  if(titles.length!==1||titles[0].tagName!=='H2'||!titles[0].id
      ||dialog.getAttribute('aria-labelledby')!==titles[0].id)
    return {status:'blocked',rejectionStage:'title'};
  if(lists.length!==1||items.length!==1||items[0].tagName!=='LI'
      ||!items[0].classList.contains('break-all')||!visible(items[0])||items[0].textContent!==workspace)
    return {status:'blocked',rejectionStage:'path'};
  if(buttons.length!==2||trust.length!==1||cancel.length!==1||trust[0].disabled
      ||trust[0].getAttribute('aria-disabled')==='true'
      ||form.querySelectorAll('input,textarea,[contenteditable="true"]').length!==0)
    return {status:'blocked',rejectionStage:'controls'};
  const button=trust[0],rect=button.getBoundingClientRect();
  if(![rect.left,rect.top,rect.width,rect.height].every(Number.isFinite))return {status:'blocked',rejectionStage:'hit'};
  const x=rect.left+rect.width/2,y=rect.top+rect.height/2,hit=document.elementFromPoint(x,y);
  if(!hit||!button.contains(hit))return {status:'blocked',rejectionStage:'hit'};
  if(held&&(held.dialog!==dialog||held.form!==form||held.title!==titles[0]
      ||held.item!==items[0]||held.button!==button||held.left!==rect.left||held.top!==rect.top
      ||held.width!==rect.width||held.height!==rect.height))return {status:'blocked',rejectionStage:'identity'};
  return {status:'proved',dialog,form,title:titles[0],item:items[0],button,
    left:rect.left,top:rect.top,width:rect.width,height:rect.height};
}
async function run(page,guard,deadline,authority,seal=()=>{}) {
  const receipt={status:'blocked',clickAttempted:false,clickCompleted:false};
  let held;
  const reject=stage=>{receipt.rejectionStage=stage;return false;};
  const owned=async()=>{
    if(Date.now()>=deadline)return reject('deadline');
    if(await authority.verify()!==true)return reject('authority');
    if(await guard()!==true)return reject('guard');
    if(Date.now()>=deadline)return reject('deadline');
    if(await authority.verify()!==true)return reject('authority');
    return Date.now()<deadline||reject('deadline');
  };
  const sampled=result=>{
    if(result.status==='proved')return true;
    return reject(result.rejectionStage);
  };
  try {
    if(!authority||typeof authority.workspace!=='string'||!authority.workspace
        ||typeof authority.verify!=='function'){reject('authority');return receipt;}
    if(!await owned())return receipt;
    held=await page.evaluateHandle(sample,{workspace:authority.workspace,held:null});
    const first=await held.evaluate(e=>({status:e.status,rejectionStage:e.rejectionStage}));
    if(first.status==='absent'){receipt.status='absent';return receipt;}
    if(!sampled(first)||!await owned())return receipt;
    await new Promise(resolve=>setTimeout(resolve,Math.min(100,Math.max(0,deadline-Date.now()))));
    if(!await owned())return receipt;
    const current=await page.evaluate(sample,{workspace:authority.workspace,held});
    if(!sampled(current)||!await owned())return receipt;
    const button=await held.evaluateHandle(e=>e.button);
    try {
      const final=await page.evaluate(sample,{workspace:authority.workspace,held});
      if(!sampled(final)||!await owned())return receipt;
      seal();
      receipt.clickAttempted=true;receipt.status='action-uncertain';
      await button.asElement().click({position:{x:final.width/2,y:final.height/2},
        timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
      receipt.clickCompleted=true;
      if(!await owned()){receipt.status='blocked';return receipt;}
      receipt.status='completed';return receipt;
    } finally {await button.dispose();}
  } catch {
    if(receipt.status==='completed')receipt.status='blocked';
    reject(Date.now()>=deadline?'deadline':'query');return receipt;
  } finally {if(held)await held.dispose();authority?.close?.();}
}
exports.sample=sample;
exports.run=run;

exports.authority=authority;
