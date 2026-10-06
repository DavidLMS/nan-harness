'use strict';
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const source=fs.readFileSync(path.join(__dirname,'codex-macos-selected-project.cjs'),'utf8');
const check=/const CHECK='([^']+)'/.exec(source)[1];
const sandbox={exports:{},getComputedStyle:()=>({display:'block',visibility:'visible'})};vm.runInNewContext(source,sandbox);
function element(tag,attrs={},children=[]){return {tagName:tag,children,isConnected:true,classList:{contains:c=>['flex','w-full','items-center','gap-1.5'].includes(c)},get lastElementChild(){return this.children.at(-1)},getBoundingClientRect:()=>({width:20,height:20}),closest:()=>null,getAttribute:k=>attrs[k]??null,hasAttribute:k=>Object.hasOwn(attrs,k),attrs};}
function fixture(){
  const icon=element('svg',{width:'17',height:'17',viewBox:'0 0 17 17',fill:'none'},[element('path',{d:check,fill:'currentColor'})]);
  const text=element('DIV'),content=element('DIV',{},[text,icon]);
  const item=element('DIV',{'data-value':'fixture-id','aria-selected':'false'},[content]);
  const other=element('DIV',{'data-value':'other','aria-selected':'true'},[element('DIV',{},[element('DIV')])]);
  const items=[item,other],list=element('DIV');list.querySelectorAll=()=>items;
  const menu=element('DIV',{'cmdk-root':''});menu.querySelectorAll=()=>[list];
  return {menu,item,other,icon,items,sample:()=>sandbox.exports.sample({menu,projectId:'fixture-id'})};
}
let count=0;function test(name,run){run();count++;console.log('PASS '+name);}
test('source check identifies selected ID independently of keyboard highlight',()=>{const f=fixture();assert.equal(f.sample().status,'observed');assert.equal(f.sample().sendAuthorized,false);f.item.attrs['aria-selected']='true';f.other.attrs['aria-selected']='false';assert.equal(f.sample().status,'observed');});
test('highlight without fixed right check never selects project',()=>{const f=fixture();f.item.attrs['aria-selected']='true';f.icon.attrs.viewBox='0 0 20 20';assert.equal(f.sample().status,'blocked');});
test('same label other ID cannot match private selected ID',()=>{const f=fixture();f.item.attrs['data-value']='other';assert.equal(f.sample().status,'blocked');});
test('two selected checks or duplicate IDs reject ambiguity',()=>{for(const kind of ['checks','ids']){const f=fixture();if(kind==='checks')f.other.children=[f.item.children[0]];else f.other.attrs['data-value']='fixture-id';assert.equal(f.sample().status,'blocked');}});
test('nested copied left icon cannot masquerade as source right icon',()=>{const f=fixture();f.item.children[0].children=[f.icon,element('DIV')];assert.equal(f.sample().status,'blocked');});
test('bounded menu and cloud ID reject',()=>{const f=fixture();f.items.push(...Array(31).fill(f.other));assert.equal(f.sample().status,'blocked');assert.equal(sandbox.exports.sample({menu:f.menu,projectId:'g-p-fixture'}).status,'blocked');});
console.log(count+' synthetic selected-project fixture groups passed');
