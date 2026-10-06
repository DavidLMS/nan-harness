// Synthetic retained-control settling; no browser or vendor process is launched.
const assert=require('node:assert/strict');
const {ordinaryClick}=require('./codex-dom.cjs');
async function trial(scenario) {
  let samples=0,guards=0,clicks=0,attempts=0,disposed=0;
  const phases=[];
  const handle={
    evaluate:async()=>{
      samples++;
      if(scenario==='overlay'&&samples===2)return {blocked:'foreign-overlay'};
      const left=scenario==='never-stable'?samples*10:samples===1?0:10;
      return {rect:[left,0,20,20],points:scenario==='lost-hit'&&samples===2?[]:[{x:10,y:10}]};
    },
    click:async options=>{
      clicks++;assert.equal(options.force,undefined);
      if(scenario==='dispatch-uncertain')throw Error('synthetic transport timeout');
    },
    dispose:async()=>{disposed++;}
  };
  const locator={count:async()=>scenario==='duplicate'&&samples>=2?2:1,
    isEnabled:async()=>true,elementHandle:async()=>handle,
    evaluate:async()=>!(scenario==='replaced'&&samples>=2)};
  const guard=async()=>{guards++;return !(scenario==='ownership-lost'&&samples>=2);};
  const deadline=Date.now()+(scenario==='never-stable'?250:1500);
  let accepted=false,error;
  try {accepted=await ordinaryClick(locator,guard,deadline,()=>{attempts++;},guard,p=>phases.push(p));}
  catch(caught){error=caught;}
  assert.equal(disposed,1);
  return {accepted,error,clicks,attempts,samples,phases};
}
(async()=>{
  const settled=await trial('settled');
  assert.equal(settled.accepted,true);
  assert.equal(settled.clicks,1);assert.equal(settled.attempts,1);
  assert.equal(settled.phases.filter(p=>p==='sample-second').length,2);
  for(const scenario of ['overlay','lost-hit','duplicate','replaced','ownership-lost','never-stable']) {
    const result=await trial(scenario);
    assert.equal(result.accepted,false,scenario);
    assert.equal(result.clicks,0,scenario);assert.equal(result.attempts,0,scenario);
  }
  const uncertain=await trial('dispatch-uncertain');
  assert.ok(uncertain.error);assert.equal(uncertain.clicks,1);assert.equal(uncertain.attempts,1);
  console.log('PASS: retained geometry settles before one click; revoked authority and original cutoff reject');
})().catch(error=>{console.error(error);process.exitCode=1;});
