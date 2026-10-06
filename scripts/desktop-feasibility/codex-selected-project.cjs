
'use strict';
// Frozen Linux _kt/bnt -> BC/n7 check icon. Passive menu observation only.
function sample({menu,projectId}) {
  const CHECK='M12.8961 3.64101C13.1297 3.41418 13.4984 3.37523 13.7779 3.56581C14.0571 3.75635 14.1554 4.11331 14.0299 4.41347L13.9615 4.53847L7.71151 13.7045C7.59411 13.8767 7.4063 13.9877 7.19881 14.0072C6.99136 14.0267 6.78564 13.9533 6.63826 13.806L2.88826 10.056L2.79842 9.9457C2.6192 9.67407 2.64927 9.30496 2.88826 9.06581C3.12738 8.82669 3.49647 8.79676 3.76815 8.97597L3.8785 9.06581L7.03084 12.2182L12.8053 3.74941L12.8961 3.64101Z';
  const visible=e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  if(!menu||!visible(menu)||!menu.hasAttribute('cmdk-root')||typeof projectId!=='string'||!projectId||projectId.startsWith('g-p-'))return {status:'blocked',reason:'menu'};
  const lists=[...menu.querySelectorAll('[cmdk-list][role="listbox"]')].filter(visible);
  if(lists.length!==1)return {status:'blocked',reason:'list'};
  const items=[...lists[0].querySelectorAll('[cmdk-item][role="option"]')].filter(visible);
  if(items.length===0||items.length>32)return {status:'blocked',reason:'limit'};
  function check(item){
    const content=[...item.children].filter(e=>e.tagName==='DIV'&&['flex','w-full','items-center','gap-1.5'].every(c=>e.classList.contains(c)));
    if(content.length!==1)return false;
    const icon=content[0].lastElementChild;
    if(!icon||icon.tagName.toLowerCase()!=='svg'||!visible(icon)||icon.getAttribute('width')!=='17'||icon.getAttribute('height')!=='17'
      ||icon.getAttribute('viewBox')!=='0 0 17 17'||icon.getAttribute('fill')!=='none'||icon.children.length!==1)return false;
    const p=icon.children[0];return p.tagName.toLowerCase()==='path'&&p.getAttribute('d')===CHECK&&p.getAttribute('fill')==='currentColor';
  }
  const selected=items.filter(check),matches=items.filter(e=>e.getAttribute('data-value')===projectId);
  if(selected.length!==1||matches.length!==1||selected[0]!==matches[0])return {status:'blocked',reason:'selected-id',selectedItemCount:selected.length,matchingItemCount:matches.length};
  return {status:'observed',selectedItemCount:1,matchingItemCount:1,selectedIdCorrelated:true,diagnosticsOnly:true,sendAuthorized:false};
}
exports.sample=sample;
