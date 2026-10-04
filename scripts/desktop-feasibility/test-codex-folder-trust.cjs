'use strict';
const assert=require('node:assert/strict');
const {sample,run}=require('./codex-folder-trust.cjs');
function fixture(options={}) {
 const node=(text='',tagName='DIV')=>({textContent:text,tagName,isConnected:true,disabled:false,
  classList:{contains:t=>t==='break-all'},getAttribute:()=>null,
  getBoundingClientRect:()=>({left:10,top:10,width:80,height:30}),contains:e=>e===button,
  querySelectorAll:()=>[]});
 const title=node('Trust this folder?','H2'),item=node(options.path??'/private/owned/workspace','LI');
 title.id='source-title';
 title.getBoundingClientRect=()=>({left:0,top:0,width:0,height:0});
 const list=node();list.children=options.duplicate?[item,item]:[item];
 const button=node('Trust folder','BUTTON');button.disabled=!!options.disabled;button.getAttribute=k=>k==='type'?'submit':null;
 const cancel=node('Cancel','BUTTON');cancel.getAttribute=k=>k==='type'?'button':null;
 const form=node();form.querySelectorAll=s=>s==='h2.contents'?[title]:s==='ul.flex.flex-col.select-text'?[list]:s==='button'?(options.duplicateButton?[button,button,cancel]:[button,cancel]):[];
 const dialog=node();dialog.getAttribute=k=>k==='role'?'dialog':k==='aria-labelledby'?'source-title':null;dialog.querySelectorAll=()=>[form];
 global.document={querySelectorAll:()=>options.multiple?[dialog,dialog]:[dialog],elementFromPoint:()=>options.foreignHit?node():button};
 global.getComputedStyle=e=>({display:e===title?(options.titleNone?'none':'contents'):'block',
  visibility:e===title&&options.titleHidden?'hidden':'visible'});
 if(options.itemHidden)item.getBoundingClientRect=()=>({left:0,top:0,width:0,height:0});
 return {button,dialog,form,title,item};
}
for(const options of [{path:'/private/other'}, {duplicate:true},{multiple:true},{disabled:true},{foreignHit:true},{duplicateButton:true},{titleNone:true},{titleHidden:true},{itemHidden:true}]) {
 fixture(options);assert.equal(sample({workspace:'/private/owned/workspace',held:null}).status,'blocked');
}
fixture({path:'/private/OTHER'});assert.equal(sample({workspace:'/private/owned/workspace',held:null}).rejectionStage,'path');
fixture({disabled:true});assert.equal(sample({workspace:'/private/owned/workspace',held:null}).rejectionStage,'controls');
fixture();const proved=sample({workspace:'/private/owned/workspace',held:null});assert.equal(proved.status,'proved');
fixture();assert.equal(sample({workspace:'/private/owned/workspace',held:proved}).rejectionStage,'identity');
async function scenario({changed=false,guardLost=false,uncertain=false,expired=false}={}) {
 fixture();let clicks=0,proofs=0,disposed=0;
 const handle=value=>({evaluate:async fn=>fn(value),evaluateHandle:async fn=>handle(fn(value)),dispose:async()=>disposed++,asElement:()=>({click:async()=>{clicks++;if(uncertain)throw Error('private');}})});
 const page={evaluateHandle:async(fn,arg)=>handle(fn(arg)),evaluate:async(fn,arg)=>{
  if(changed)fixture();return fn({...arg,held:arg.held?await arg.held.evaluate(x=>x):null});}};
 const guard=async()=>{proofs++;return !guardLost||proofs<3;};
 const result=await run(page,guard,expired?Date.now()-1:Date.now()+2000,
  {workspace:'/private/owned/workspace',verify:async()=>true});
 return {result,clicks,disposed};
}
(async()=>{
 let r=await scenario();assert.equal(r.result.status,'completed');assert.equal(r.clicks,1);
 for(const [o,stage] of [[{changed:true},'identity'],[{guardLost:true},'guard'],[{expired:true},'deadline']]){r=await scenario(o);assert.equal(r.clicks,0);assert.equal(r.result.rejectionStage,stage);}
 r=await scenario({uncertain:true});assert.equal(r.clicks,1);assert.equal(r.result.status,'action-uncertain');assert.equal(r.result.rejectionStage,'query');
 assert.deepEqual(Object.keys(r.result).sort(),['clickAttempted','clickCompleted','rejectionStage','status']);
 console.log('folder trust source/identity/action fixtures PASS');
})().catch(e=>{console.error(e);process.exitCode=1;});
(async()=>{
 const result=await run({},async()=>true,Date.now()+1000,{workspace:'',verify:()=>false});
 assert.equal(result.rejectionStage,'authority');assert.equal(result.clickAttempted,false);
})().catch(e=>{console.error(e);process.exitCode=1;});
const authority=require('./codex-folder-trust.cjs').authority;
function filesystem({symlink=false,changed=false}={}) {
 let mutable=false,closed=0;
 const stat=()=>({dev:1n,ino:mutable&&changed?3n:2n,birthtimeNs:1n,uid:BigInt(process.getuid()),mode:0o700n,
  isDirectory:()=>true,isSymbolicLink:()=>symlink});
 const io={lstatSync:stat,fstatSync:stat,openSync:()=>9,closeSync:()=>closed++,
  realpathSync:{native:p=>p}};
 return {io,change:()=>{mutable=true;},closed:()=>closed};
}
let f=filesystem();let a=authority('/private/owned/workspace',f.io,'darwin');assert.equal(a.verify(),true);a.close();assert.equal(f.closed(),1);
f=filesystem({symlink:true});a=authority('/private/owned/workspace',f.io,'darwin');assert.equal(a.verify(),false);
f=filesystem({changed:true});a=authority('/private/owned/workspace',f.io,'darwin');f.change();assert.equal(a.verify(),false);a.close();
f=filesystem();a=authority('\\\\?\\C:\\private\\owned\\workspace',f.io,'win32');assert.equal(a.workspace,'C:\\private\\owned\\workspace');assert.equal(a.verify(),true);a.close();
assert.equal(authority('\\\\server\\share',f.io,'win32').verify(),false);
