'use strict';
// Connection setup is read-only. A failed handshake may be retried before any
// page is admitted, with fresh native custody and one unchanged caller budget.
exports.connect = async function connect(open, owned, deadline, pause=ms=>new Promise(r=>setTimeout(r,ms)), now=Date.now) {
  for(let attempt=0;attempt<3;attempt++) {
    if(now()>=deadline || !owned() || now()>=deadline)throw new Error('connection ownership');
    let browser;
    try { browser=await open(Math.max(1,Math.min(8000,deadline-now()))); }
    catch {
      if(attempt===2 || now()>=deadline || !owned())throw new Error('connection unavailable');
      await pause(Math.min(100,Math.max(0,deadline-now())));
      continue;
    }
    if(now()>=deadline || !owned() || now()>=deadline)throw new Error('connection ownership');
    return browser;
  }
};
