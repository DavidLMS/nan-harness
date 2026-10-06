'use strict';
// Source-declared projectless -> selected local toolbar transition only.
// Caller retains original CDP document/home/editor and one consumed selection.
function prepare(previous) {
 if(!previous||previous.control!==previous.button||!previous.home.contains(previous.button)
   ||previous.button.closest('div._ActiveProjectSelectorTrigger_1jl81_1')
   ||previous.home.querySelectorAll('button[data-clear-project-button]').length
   ||previous.button.querySelectorAll('[data-project-selector-icon]').length)return false;
 previous.declaredProjectlessControl=true;return true;
}
function capture(previous,{projectName,originalMenu}) {
 const visible=e=>{if(!e)return false;const r=e.getBoundingClientRect(),s=getComputedStyle(e);return e.isConnected&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&!e.closest('[inert]');};
 if(!previous||previous.declaredProjectlessControl!==true||previous.controlTransitionConsumed===true||typeof projectName!=='string'||!projectName||projectName.length>4096
   ||!visible(previous.home)||!visible(previous.editor)||previous.control!==previous.button
   ||previous.button.isConnected||visible(originalMenu))return null;
 const home=previous.home,editor=previous.editor;
 const homes=[...document.querySelectorAll('[data-codex-composer-root][data-composer-placement="home"]')].filter(visible);
 const editors=[...home.querySelectorAll('.ProseMirror[contenteditable="true"]')].filter(visible);
 const controls=[...home.querySelectorAll('[data-composer-navigation-target="workspace-project"]')].filter(visible);
 if(homes.length!==1||homes[0]!==home||editors.length!==1||editors[0]!==editor||controls.length!==1
   ||[...document.querySelectorAll('[cmdk-root],[role="dialog"],[role="alertdialog"],[aria-modal="true"],[role="menu"]')].some(visible))return null;
 const button=controls[0],wrapper=button.closest('div._ActiveProjectSelectorTrigger_1jl81_1');
 if(!wrapper||!home.contains(wrapper)||wrapper.querySelectorAll('[data-composer-navigation-target="workspace-project"]').length!==1
   ||button.tagName!=='BUTTON'||button.closest('button')!==button||button.disabled||button.getAttribute('aria-disabled')==='true'
   ||button.getAttribute('data-slot')!=='popover-trigger'||button.getAttribute('aria-haspopup')!=='dialog'
   ||button.getAttribute('aria-expanded')!=='false'||button.getAttribute('aria-label')!==`Change project: ${projectName}`
   ||button.querySelectorAll('[data-project-selector-icon]').length!==1)return null;
 const clear=[...wrapper.querySelectorAll('button[data-clear-project-button]')];
 const buttons=[...wrapper.querySelectorAll('button')];
 if(buttons.length!==2||!buttons.includes(button)||!buttons.includes(clear[0])
   ||home.querySelectorAll('button[data-clear-project-button]').length!==1)return null;
 if(clear.length!==1||clear[0]===button||clear[0].type!=='button'||clear[0].tabIndex!==-1
   ||clear[0].getAttribute('aria-label')!=="Don't work in a project"
   ||!clear[0].classList.contains('_ActiveProjectSelectorTriggerClearButton_1jl81_45'))return null;
 previous.controlTransitionConsumed=true;
 return {home,editor,control:button,button};
}
module.exports={prepare,capture};
