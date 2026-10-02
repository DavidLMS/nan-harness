// Public UI only. This module never reads accounts or changes application stores.
const TOKENS = ['relative', 'flex', 'h-full', 'min-h-0', 'w-full', 'flex-col',
  'bg-transparent', 'tracking-normal', 'text-default', 'select-text'];
const SCOPE = 'div' + TOKENS.map(token => `[class~="${token}"]`).join('');
const GROUP = 'input[type="radio"][name="conversational-onboarding-inline-role"]';
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));

// Standalone browser callback: no Node helpers or application internals.
function sample(control) {
  if (!['LABEL','BUTTON'].includes(control.tagName)) return null;
  if (!control.isConnected || control.ownerDocument !== document || control.closest('[inert]')) return null;
  const visible = e => { const r = e.getBoundingClientRect(); const s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.display !== 'none' && s.visibility !== 'hidden'; };
  if ([...document.querySelectorAll('[role="dialog"],[aria-modal="true"],[role="alertdialog"],[role="menu"]')].some(visible)) return null;
  for (let e = control, depth = 0; e; e = e.parentElement) {
    if (++depth > 64 || getComputedStyle(e).pointerEvents === 'none') return null;
  }
  if (!visible(control) || control.disabled || control.getAttribute('aria-disabled') === 'true') return null;
  const r = control.getBoundingClientRect();
  const points = [];
  for (const fy of [0.25, 0.5, 0.75]) for (const fx of [0.25, 0.5, 0.75]) {
    const x = r.left + control.clientLeft + control.clientWidth * fx;
    const y = r.top + control.clientTop + control.clientHeight * fy;
    if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) continue;
    const front = document.elementFromPoint(x, y);
    if (front === control || control.contains(front)) points.push({x: x-r.left-control.clientLeft, y:y-r.top-control.clientTop});
  }
  return {rect: [r.left,r.top,r.width,r.height], points};
}
function candidate(a, b) {
  return a && b && JSON.stringify(a.rect) === JSON.stringify(b.rect)
    && a.points.find(p => b.points.some(q => p.x === q.x && p.y === q.y));
}
exports.run = async function(page, ownerGuard, deadline) {
  const maxWaitMs = deadline - Date.now();
  const originalUrl = page.url();
  const ownedEndpoint = () => {
    const fail = reason => { facts.roleProofFailure=reason; return false; };
    try {
      if (typeof ownerGuard !== 'function' || ownerGuard() !== true) return fail('ownership-lost');
      // Read renderer identity after the synchronous native proof, which can
      // block while a page or route changes. Never reuse its earlier snapshot.
      const browser = page.context().browser();
      const pages = browser?.contexts().flatMap(context => context.pages());
      if (!pages) return fail('query-failed');
      if (pages.length !== 1) return fail('page-count');
      if (pages[0] !== page) return fail('page-changed');
      if (page.url() !== originalUrl) return fail('url-changed');
      return true;
    } catch { return fail('query-failed'); }
  };
  const facts = {schemaVersion:1, mechanism:'codex-public-onboarding', diagnosticsOnly:true,
    stage:'session', errorCategory:null, conversationalScope:false, engineeringControl:false,
    roleClickAttempted:false, roleClickCompleted:false, engineeringChecked:false,
    continueControl:false, continueClickAttempted:false, continueClickCompleted:false,
    roleScopeAbsent:false, roleProofFailure:'unmeasured', sessionProofFailure:'unmeasured'};
  const stop = category => { facts.errorCategory=category; return facts; };
  const sessionFailure = typeof ownerGuard !== 'function' ? 'guard-missing'
    : !Number.isFinite(deadline) || !Number.isFinite(maxWaitMs) || maxWaitMs > 25000 ? 'deadline-invalid'
    : maxWaitMs < 1 ? 'deadline-expired'
    : process.platform !== 'win32' ? 'platform'
    : process.env.GITHUB_ACTIONS !== 'true' || process.env.RUNNER_ENVIRONMENT !== 'github-hosted'
      || process.env.RUNNER_OS !== 'Windows' ? 'host-policy'
    : process.env.NANH_CODEX_PUBLIC_ONBOARDING !== 'engineering' ? 'onboarding-policy' : null;
  if (sessionFailure !== null) {
    facts.sessionProofFailure=sessionFailure;
    return stop('invalid-session');
  }
  let scope, fieldset, radio, label, button;
  async function proof(needChecked=false) {
    const fail = reason => { facts.roleProofFailure=reason; return false; };
    facts.roleProofFailure='unmeasured';
    if (Date.now() >= deadline) return fail('deadline-expired');
    if (!ownedEndpoint()) return false;
    if (Date.now() >= deadline) return fail('deadline-expired');
    const legends = page.locator('fieldset > legend:visible').filter({hasText:/^Select the kind of work you do$/});
    if (await legends.count() !== 1) return fail('legend-count');
    fieldset = legends.locator('..');
    if (await fieldset.locator(GROUP).count() < 1) return fail('group-absent');
    scope = fieldset.locator(`xpath=ancestor::div[${TOKENS.map(t=>`contains(concat(' ', normalize-space(@class), ' '), ' ${t} ')`).join(' and ')}][1]`);
    if (await scope.count() !== 1) return fail('scope-count');
    // The same public role page also has a footer fieldset for the optional
    // personalized-suggestions checkbox. Only the live role group is unique.
    const roleFieldsets = scope.locator('fieldset:visible')
      .filter({has: page.locator('legend').filter({hasText:/^Select the kind of work you do$/})})
      .filter({has: page.locator(GROUP)});
    if (await roleFieldsets.count() !== 1) return fail('fieldset-count');
    if (await scope.getByRole('button',{name:/^(Log in|Sign in|Continue with Google|Continue with Apple)$/i}).count() !== 0) return fail('login-present');
    radio = fieldset.locator(`${GROUP}[value="engineering"]`);
    if (await radio.count() !== 1) return fail('engineering-count');
    label = fieldset.locator('label').filter({hasText:/^Engineering$/});
    if (await label.count() !== 1) return fail('label-count');
    if (!await label.evaluate((e, group) => {
      const inputs=[...e.ownerDocument.querySelectorAll(group+'[value="engineering"]')];
      return inputs.length===1 && inputs[0].labels?.length===1 && inputs[0].labels[0]===e && e.innerText.trim()==='Engineering';
    },GROUP)) return fail('label-association');
    if (!needChecked && !await radio.isEnabled()) return fail('engineering-disabled');
    if (needChecked && (!await radio.isChecked() || await fieldset.locator(GROUP+':checked').count() !== 1)) return fail('checked-mismatch');
    if (!ownedEndpoint()) return false;
    if (Date.now() >= deadline) return fail('deadline-expired');
    return true;
  }
  async function click(control, reprove, before, after) {
    if (!await reprove()) return false;
    const handle = await control.elementHandle();
    if (!handle) return false;
    try {
      const first = await handle.evaluate(sample);
      await wait(Math.min(100,Math.max(0,deadline-Date.now())));
      if (!await reprove() || !await control.evaluate((e, held)=>e===held,handle)) return false;
      const point = candidate(first,await handle.evaluate(sample));
      if (!point) return false;
      if (!await reprove() || !await control.evaluate((e, held)=>e===held,handle)) return false;
      const final=await handle.evaluate(sample);
      if (!candidate(first,final) || !final.points.some(p=>p.x===point.x&&p.y===point.y)) return false;
      if (!ownedEndpoint()) return false;
      if (Date.now() >= deadline) { facts.roleProofFailure='deadline-expired'; return false; }
      facts[before]=true;
      await handle.click({position:point,timeout:Math.max(1,Math.min(2000,deadline-Date.now()))});
      facts[after]=true;
      return true;
    } finally { await handle.dispose(); }
  }
  try {
    facts.stage='role-proof';
    // Frozen conversational parent disables controls during its pending work.
    // Only that positively matched, disabled control is a pollable startup state.
    while (!await proof()) {
      if (facts.roleProofFailure !== 'engineering-disabled' || Date.now() >= deadline)
        return stop('scope-not-matched');
      await wait(Math.min(100, Math.max(0, deadline-Date.now())));
    }
    facts.conversationalScope=true; facts.engineeringControl=true;
    if (await fieldset.locator(GROUP+':checked').count() !== 0) return stop('role-already-selected');
    facts.stage='role-action';
    if (!await click(label,()=>proof(), 'roleClickAttempted','roleClickCompleted')) return stop('action-blocked');
    facts.stage='role-readback';
    while (Date.now()<deadline && !await proof(true)) {
      if (!ownedEndpoint()) return stop('ownership-lost');
      await wait(100);
    }
    if (!await proof(true)) return stop('role-readback-failed');
    facts.engineeringChecked=true;
    button=scope.getByRole('button',{name:'Continue',exact:true});
    while (Date.now()<deadline && await button.count()===1 && !await button.isEnabled()) {
      if (!await proof(true)) return stop('role-readback-failed');
      await wait(100);
    }
    if (await button.count() !== 1 || !await button.isEnabled()
        || !await button.evaluate(e=>e.tagName==='BUTTON')) return stop('continue-not-matched');
    facts.continueControl=true;
    const continueProof=async()=>await proof(true)&&await button.count()===1&&await button.isEnabled();
    facts.stage='continue-action';
    if (!await click(button,continueProof,'continueClickAttempted','continueClickCompleted')) return stop('action-blocked');
    facts.stage='scope-transition';
    while (Date.now()<deadline) {
      if (!ownedEndpoint()) return stop('ownership-lost');
      if (await page.locator(GROUP).count()===0) { facts.roleScopeAbsent=true;facts.stage='stopped-after-role';return facts; }
      await wait(100);
    }
    return stop('scope-remained');
  } catch { return stop(facts.roleClickAttempted || facts.continueClickAttempted ? 'action-uncertain':'observation-failed'); }
};
exports.sample = sample;
exports.candidate = candidate;
exports.scopeFingerprint = SCOPE;
