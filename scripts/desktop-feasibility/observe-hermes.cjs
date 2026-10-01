// Read-only renderer observation. Never capture text, URLs, screenshots, or DOM.
const fs = require('node:fs');
const { chromium } = require('../../.github/web-check/node_modules/playwright');
const [port, owner, output] = process.argv.slice(2);
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
(async () => {
  const deadline = Date.now() + 120000;
  let browser;
  while (Date.now() < deadline) {
    try {
      if (!browser) {
        if (!ownedEndpoint()) { await new Promise(r => setTimeout(r, 250)); continue; }
        facts.endpointOwned = true;
        browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 2000 });
        facts.attached = true;
      }
      for (const context of browser.contexts()) for (const page of context.pages()) {
        const observation = await page.evaluate(() => {
          const visible = e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
          const fields = [...document.querySelectorAll('textarea,[contenteditable="true"],input[type="text"]')].filter(visible);
          const value = fields.length === 1 ? (fields[0].value ?? fields[0].textContent) : '';
          return { uniqueComposer: fields.length === 1,
            inputReadback: value === 'Check this connection',
            syntheticTextPresent: /NAN CHECK RESPONSE(?: (?:APPLE|BREAD|CHAIR|DREAM|EAGLE|FIELD|GREEN|HOUSE|ISLAND|JUICE|KITE|LEMON|MOON|NORTH|OCEAN|PAPER)){32}/.test(document.body.innerText) };
        });
        for (const key of Object.keys(observation)) facts[key] ||= observation[key];
      }
      saveFacts();
      await new Promise(r => setTimeout(r, 100));
    } catch { await new Promise(r => setTimeout(r, 250)); }
    try { process.kill(Number(owner), 0); } catch { break; }
  }
  saveFacts();
  // Do not browser.close(): the checker owns application lifetime.
  process.exit(0);
})().catch(() => { saveFacts(); process.exit(1); });
