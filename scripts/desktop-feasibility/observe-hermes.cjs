// Owned renderer observation and opt-in input. Publish only closed facts.
const fs = require('node:fs');
const { chromium } = require('../../.github/web-check/node_modules/playwright');
const drive = process.argv[2] === '--drive';
const request = drive ? JSON.parse(fs.readFileSync(process.argv[3], 'utf8')) : null;
const connection = drive ? JSON.parse(fs.readFileSync(request.connectionPath, 'utf8')) : null;
const [port, owner, output] = drive
  ? [String(connection.port), String(connection.launcherPid), process.argv[4]]
  : process.argv.slice(2);
const facts = { schemaVersion: 1, mechanism: 'hermes-cdp', endpointOwned: false,
  attached: false, uniqueComposer: false, inputReadback: false, syntheticTextPresent: false };
function saveFacts() {
  const temporary = `${output}.tmp`;
  fs.writeFileSync(temporary, JSON.stringify(facts) + '\n', { mode: 0o600 });
  fs.renameSync(temporary, output);
}
saveFacts();
function descendant(pid) {
  for (let depth = 0; depth < 32 && pid > 1; depth++) {
    if (String(pid) === owner) return true;
    const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
    pid = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]);
  }
  return false;
}
function ownedEndpoint() {
  // Associate the LISTEN socket inode with a child of the nanh launcher.
  const hexPort = Number(port).toString(16).toUpperCase().padStart(4, '0');
  const sockets = fs.readFileSync('/proc/net/tcp', 'utf8').trim().split('\n').slice(1)
    .map(line => line.trim().split(/\s+/))
    .filter(row => row[1] === `0100007F:${hexPort}` && row[3] === '0A');
  if (sockets.length !== 1) return false;
  const inode = `socket:[${sockets[0][9]}]`;
  for (const pid of fs.readdirSync('/proc').filter(x => /^\d+$/.test(x))) {
    try {
      if (!descendant(Number(pid))) continue;
      for (const fd of fs.readdirSync(`/proc/${pid}/fd`)) {
        if (fs.readlinkSync(`/proc/${pid}/fd/${fd}`) === inode) return true;
      }
    } catch { /* Processes can exit between enumeration and inspection. */ }
  }
  return false;
}
function delay(ms) { return new Promise(resolve => setTimeout(resolve, ms)); }
async function driveDom() {
  Object.assign(facts, { mechanism: 'hermes-playwright-dom', targetVerified: false,
    responseVerified: false, inputSubmitted: false, errorCategory: 'unclassified',
    playwrightVersion: require('../../.github/web-check/node_modules/playwright/package.json').version,
    observedRuntimeVersion: null });
  saveFacts();
  const exactKeys = (value, keys) => value && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
  if (!exactKeys(request, ['connectionPath', 'ownerPid', 'prompt', 'expectedMarker', 'timeoutMs']) ||
      !exactKeys(connection, ['schemaVersion', 'port', 'launcherPid']) || connection.schemaVersion !== 1 ||
      typeof request.connectionPath !== 'string' || request.connectionPath.length > 4096 || !Number.isInteger(connection.port) || connection.port < 1 || connection.port > 65535 ||
      !Number.isInteger(connection.launcherPid) || connection.launcherPid <= 1 ||
      !Number.isInteger(request.ownerPid) || request.ownerPid <= 1 ||
      request.prompt !== 'Check this connection' ||
      typeof request.expectedMarker !== 'string' || request.expectedMarker.length < 32 ||
      request.expectedMarker.length > 2048 || !Number.isInteger(request.timeoutMs) ||
      request.timeoutMs < 1 || request.timeoutMs > 30000) {
    facts.errorCategory = 'invalid-request'; saveFacts(); return;
  }
  let ancestor = connection.launcherPid;
  let launcherOwned = false;
  for (let depth = 0; depth < 32 && ancestor > 1; depth++) {
    if (ancestor === request.ownerPid) { launcherOwned = true; break; }
    try { const stat = fs.readFileSync(`/proc/${ancestor}/stat`, 'utf8');
      ancestor = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]);
    } catch { break; }
  }
  if (!launcherOwned) { facts.errorCategory = 'launcher-unowned'; saveFacts(); return; }
  const deadline = Date.now() + request.timeoutMs;
  while (!ownedEndpoint() && Date.now() < deadline) await delay(100);
  if (!ownedEndpoint()) { facts.errorCategory = 'endpoint-unowned'; saveFacts(); return; }
  facts.endpointOwned = true;
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`,
    { timeout: Math.min(2000, request.timeoutMs), noDefaults: true });
  facts.attached = true;
  const version = browser.version();
  if (/^(?:Chrome\/)?[0-9]+(?:\.[0-9]+){1,3}$/.test(version)) facts.observedRuntimeVersion = version.replace(/^Chrome\//, '');
  const pages = browser.contexts().flatMap(context => context.pages())
    .filter(page => { try { const url = new URL(page.url());
      return url.protocol === 'file:' && /\/resources\/app\.asar(?:\.unpacked)?\/dist\/index\.html$/.test(decodeURIComponent(url.pathname));
    } catch { return false; } });
  if (pages.length !== 1 || !ownedEndpoint()) {
    facts.errorCategory = 'target-ambiguous'; saveFacts(); return;
  }
  const page = pages[0];
  const session = await page.context().newCDPSession(page);
  const target = (await session.send('Target.getTargetInfo')).targetInfo;
  if (target.type !== 'page' || target.url !== page.url()) {
    facts.errorCategory = 'target-invalid'; saveFacts(); return;
  }
  facts.targetVerified = true;
  // Select only a single visible editable composer, never set application stores.
  while (Date.now() < deadline && !(await page.evaluate(() => { const fields = [...document.querySelectorAll('textarea,[contenteditable="true"],input[type="text"]')]; return fields.filter(e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && !e.disabled && !e.readOnly; }).length === 1; })))
    await delay(100);
  const candidates = page.locator('textarea:visible,[contenteditable="true"]:visible,input[type="text"]:visible');
  if (await candidates.count() !== 1 || !await candidates.isEditable()) {
    facts.errorCategory = 'composer-ambiguous'; saveFacts(); return;
  }
  facts.uniqueComposer = true;
  // Frozen Hermes assistant-message.tsx MessagePrimitive.Root exposes data-role=assistant.
  const assistant = page.locator('[data-role="assistant"]:visible');
  if (await assistant.filter({ hasText: request.expectedMarker }).count() !== 0) {
    facts.errorCategory = 'stale-response'; saveFacts(); return;
  }
  await candidates.fill(request.prompt, { timeout: Math.max(1, deadline - Date.now()) });
  facts.inputReadback = await candidates.evaluate((e, prompt) =>
    (e.value ?? e.textContent) === prompt, request.prompt);
  if (!facts.inputReadback || !ownedEndpoint()) {
    facts.errorCategory = 'input-mismatch'; saveFacts(); return;
  }
  facts.inputSubmitted = true; saveFacts();
  await candidates.press('Enter', { timeout: Math.max(1, deadline - Date.now()) });
  while (Date.now() < deadline) {
    const matching = assistant.filter({ hasText: request.expectedMarker });
    if (await matching.count() === 1 && await matching.evaluate((e, marker) =>
        e.innerText.includes(marker), request.expectedMarker)) {
      facts.responseVerified = true; facts.syntheticTextPresent = true;
      facts.errorCategory = null; saveFacts(); return;
    }
    if (!ownedEndpoint()) break;
    await delay(100);
  }
  facts.errorCategory = 'response-timeout'; saveFacts();
}
(async () => {
  if (drive) { await driveDom(); process.exit(facts.responseVerified ? 0 : 1); }
  const deadline = Date.now() + 120000;
  let browser;
  while (Date.now() < deadline) {
    try {
      if (!browser) {
        if (!ownedEndpoint()) { await delay(250); continue; }
        facts.endpointOwned = true;
        browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 2000, noDefaults: true });
        facts.attached = true;
      }
      for (const context of browser.contexts()) for (const page of context.pages()) {
        const observation = await page.evaluate(() => {
          const visible = e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
          const fields = [...document.querySelectorAll('textarea,[contenteditable="true"],input[type="text"]')].filter(visible);
          return { uniqueComposer: fields.length === 1,
            inputReadback: fields.length === 1 && (fields[0].value ?? fields[0].textContent) === 'Check this connection',
            syntheticTextPresent: [...document.querySelectorAll('[data-role="assistant"]')].some(e =>
              /NAN CHECK RESPONSE(?: (?:APPLE|BREAD|CHAIR|DREAM|EAGLE|FIELD|GREEN|HOUSE|ISLAND|JUICE|KITE|LEMON|MOON|NORTH|OCEAN|PAPER)){32}/.test(e.innerText)) };
        });
        for (const key of Object.keys(observation)) facts[key] ||= observation[key];
      }
      saveFacts(); await delay(100);
    } catch { await delay(250); }
    try { process.kill(Number(owner), 0); } catch { break; }
  }
  saveFacts(); process.exit(0);
})().catch(() => { if (drive) facts.errorCategory = 'attachment-or-action-failed'; saveFacts(); process.exit(1); });
