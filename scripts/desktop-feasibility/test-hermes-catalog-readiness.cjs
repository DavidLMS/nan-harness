'use strict';
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');
const { monitor } = require('./hermes-catalog-readiness.cjs');
const session = new EventEmitter(); let owned = true;
const evidence = monitor(session, () => owned, 'qwen3.6', 'nan');
function emit(direction, socket, value) { session.emit('Network.webSocketFrame' + direction, {requestId: socket, response:{opcode:1,payloadData:JSON.stringify(value)}}); }
const request = {jsonrpc:'2.0',id:1,method:'model.options',params:{profile:'nan',explicit_only:true}};
const response = {jsonrpc:'2.0',id:1,result:{providers:[{models:['qwen3.6']}]}};
emit('Sent','A',request); emit('Received','A',response); assert(!evidence.verified()); // pre-activation request/result
evidence.arm();
emit('Received','A',response); assert(!evidence.verified()); // cached/unobserved result
emit('Sent','A',request); emit('Received','B',response); assert(!evidence.verified());
emit('Received','A',{...response,error:{message:'PRIVATE'}}); assert(!evidence.verified());
evidence.arm(); emit('Sent','A',request); emit('Received','A',response); assert(evidence.verified());
session.emit('Network.webSocketClosed',{requestId:'A'}); assert(!evidence.verified());
emit('Sent','A',{...request,params:{profile:'foreign',explicit_only:true}}); emit('Received','A',response); assert(!evidence.verified());
evidence.arm(); emit('Sent','A',request); emit('Received','A',{...response,result:{providers:[{models:['wrong']}]}}); assert(!evidence.verified());
emit('Sent','A',request); owned=false; emit('Received','A',response); assert(!evidence.verified());
owned=true; evidence.arm(); emit('Sent','A',request); emit('Received','A',response); assert(evidence.verified());
emit('Sent','B',{...request,id:2}); assert(!evidence.verified());
evidence.arm(); emit('Sent','A',request); emit('Sent','B',{...request,id:2}); emit('Received','A',response); assert(!evidence.verified());
evidence.arm(); emit('Sent','A',request); emit('Sent','A',{...request,id:2}); emit('Received','A',response); assert(!evidence.verified());
evidence.dispose(); assert.equal(session.listenerCount('Network.webSocketFrameSent'),0);
console.log('PASS passive catalog readiness guards');
// Execute the actual serialized browser sampler without Node helper bindings.
const vm = require('node:vm');
const callback = require('./hermes-windows-ready.cjs').sample;
const doc = {elementFromPoint:()=>button};
const button = {isConnected:true,ownerDocument:doc,clientWidth:20,clientHeight:20,clientLeft:0,clientTop:0,
 closest:()=>null,matches:()=>false,getAttribute:()=>null,checkVisibility:()=>true,
 getBoundingClientRect:()=>({left:0,top:0,right:20,bottom:20,width:20,height:20}),contains:()=>false};
const sample = vm.runInNewContext('('+callback.toString()+')',{document:doc,innerWidth:100,innerHeight:100});
assert(sample(button));
doc.elementFromPoint=()=>({});assert.equal(sample(button),null);
doc.elementFromPoint=()=>button;button.ownerDocument={};assert.equal(sample(button),null);
button.ownerDocument=doc;button.matches=()=>true;assert.equal(sample(button),null);
console.log('PASS standalone browser sampler guards');
