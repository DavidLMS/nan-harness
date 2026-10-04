'use strict';
// Retain only the frozen _kt exact private local-project cmdk item.
function capture({menu,projectId}) {
  const visible=e=>{if(!e)return false;const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  if(!visible(menu)||!menu.hasAttribute('cmdk-root')||typeof projectId!=='string'||!projectId||projectId.startsWith('g-p-'))return null;
  const lists=[...menu.querySelectorAll('[cmdk-list][role="listbox"]')].filter(visible);
  if(lists.length!==1)return null;
  const matches=[...lists[0].querySelectorAll('[cmdk-item][role="option"]')].filter(e=>visible(e)&&e.getAttribute('data-value')===projectId);
  if(matches.length!==1)return null;
  return {menu,list:lists[0],item:matches[0],projectId};
}
function sample(held){
  const visible=e=>{if(!e)return false;const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
  if(!held||!visible(held.menu)||!visible(held.list)||!visible(held.item)||!held.menu.contains(held.list)||!held.list.contains(held.item))return {matched:false};
  const menus=[...document.querySelectorAll('[cmdk-root]')].filter(visible);
  const lists=[...held.menu.querySelectorAll('[cmdk-list][role="listbox"]')].filter(visible);
  const matches=[...held.list.querySelectorAll('[cmdk-item][role="option"]')].filter(e=>visible(e)&&e.getAttribute('data-value')===held.projectId);
  if(menus.length!==1||menus[0]!==held.menu||lists.length!==1||lists[0]!==held.list||matches.length!==1||matches[0]!==held.item
    ||held.item.getAttribute('aria-disabled')==='true'||held.item.hasAttribute('disabled')||held.item.getAttribute('data-disabled')==='true'
    ||held.item.getAttribute('role')!=='option'||!held.item.hasAttribute('cmdk-item'))return {matched:false};
  const r=held.item.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2;
  if(![r.left,r.top,r.width,r.height,x,y].every(Number.isFinite)||x<0||y<0||x>=innerWidth||y>=innerHeight||!held.item.contains(document.elementFromPoint(x,y)))return {matched:false};
  return {matched:true,rect:[r.left,r.top,r.width,r.height]};
}
exports.capture=capture;exports.sample=sample;
