// Bound native ancestry and listener ownership; no application data is read.
// Process-local advisory observations survive separate ancestry/listener proof objects.
const windowsProofCategoryCounts = Object.fromEntries([
  'owned', 'process-budget', 'ancestry-cycle', 'process-unavailable', 'parent-unavailable',
  'parent-reused', 'session-mismatch', 'ancestry-limit', 'listener-unavailable',
  'query-failed', 'unclassified', 'transport-timeout', 'transport-failed',
].map(category => [category, 0]));
exports.proof = function(owner, port) {
const fs = require("node:fs");
function saveWindowsProof(category) {
  const directory = process.env?.NANH_DESKTOP_QUALIFICATION_FACTS;
  if (process.env?.GITHUB_ACTIONS !== 'true' || process.env?.RUNNER_ENVIRONMENT !== 'github-hosted'
      || !directory) return;
  try {
    const path = require('node:path');
    if (!path.isAbsolute(directory) || !fs.lstatSync(directory).isDirectory()) return;
    const output = path.join(directory, `windows-proof-${process.pid}.json`);
    if (!Object.hasOwn(windowsProofCategoryCounts, category)) return;
    windowsProofCategoryCounts[category] = Math.min(4096, windowsProofCategoryCounts[category] + 1);
    const value = { schemaVersion: 1, mechanism: 'windows-endpoint-proof', diagnosticsOnly: true, category,
      categoryCounts: {...windowsProofCategoryCounts} };
    fs.writeFileSync(`${output}.tmp`, JSON.stringify(value) + '\n', { mode: 0o600 });
    fs.renameSync(`${output}.tmp`, output);
  } catch { /* A missing diagnostic never authorizes an action. */ }
}
function windowsProof(mode, value, root) {
  if (!['endpoint', 'descendant'].includes(mode) || !Number.isSafeInteger(value)
      || value <= 1 || value > (mode === 'endpoint' ? 65535 : 2147483647)
      || !Number.isSafeInteger(root) || root <= 1 || root > 2147483647) return false;
  try {
    const python = process.env?.FEASIBILITY_WINDOWS_PROOF_PYTHON;
    const native = process.env?.GITHUB_ACTIONS === 'true'
      && process.env?.RUNNER_ENVIRONMENT === 'github-hosted'
      && typeof python === 'string' && /^[A-Za-z]:[\\/]/.test(python);
    const result = require('node:child_process').execFileSync(native ? python : 'pwsh',
      native ? [`${__dirname}/endpoint-owner-windows.py`, mode, String(value), String(root)]
        : ['-NoProfile', '-NonInteractive', '-File', `${__dirname}/endpoint-owner.ps1`,
           mode, String(value), String(root)],
      { encoding: 'utf8', timeout: 8000, maxBuffer: 4096, windowsHide: true,
        stdio: ['ignore', 'pipe', 'ignore'] });
    const categories = ['true', 'process-budget', 'ancestry-cycle', 'process-unavailable', 'parent-unavailable', 'parent-reused', 'session-mismatch', 'ancestry-limit', 'listener-unavailable', 'query-failed'];
    saveWindowsProof(categories.includes(result) ? (result === 'true' ? 'owned' : result) : 'unclassified');
    return result === 'true';
  } catch (error) {
    const closed = error?.stdout?.toString();
    saveWindowsProof(closed === 'query-failed' ? 'query-failed'
      : error?.code === 'ETIMEDOUT' ? 'transport-timeout' : 'transport-failed');
    return false;
  }
}
let unixFailure='unmeasured';
let listenerShape = null;
function failureDetails() { return unixFailure === 'listener-shape' ? listenerShape : null; }
function failure() { return unixFailure; }
function descendant(pid) {
  if (process.platform === 'win32') return windowsProof('descendant', pid, Number(owner));
  unixFailure='unmeasured';
  for (let depth = 0; depth < 32 && pid > 1; depth++) {
    if (String(pid) === owner) return true;
    try { pid = parentPid(pid); }
    catch (error) { unixFailure='ancestor-query';throw error; }
  }
  if(unixFailure==='unmeasured')unixFailure='ancestor-unowned';
  return false;
}
function parentPid(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 1) return 0;
  if (process.platform === 'darwin') {
    const parent = require('node:child_process').execFileSync('/bin/ps',
      ['-o', 'ppid=', '-p', String(pid)], { encoding: 'utf8', timeout: 2000,
        maxBuffer: 4096, stdio: ['ignore', 'pipe', 'ignore'] }).trim();
    if(!/^[0-9]+$/.test(parent))unixFailure='ancestor-query';
    return /^[0-9]+$/.test(parent) ? Number(parent) : 0;
  }
  const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
  return Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]);
}
function ownedEndpoint() {
  listenerShape = null;
  if (process.platform === 'win32') return windowsProof('endpoint', Number(port), Number(owner));
  unixFailure='unmeasured';
  if (process.platform === 'darwin') {
    let stage='listener-query';
    // lsof selects listeners by port; reject wildcard/non-loopback bindings.
    try {
      const listing = require('node:child_process').execFileSync('/usr/sbin/lsof',
        // Recent lsof versions no longer emit the file descriptor implicitly.
        ['-nP', '-a', `-iTCP:${port}`, '-sTCP:LISTEN', '-Fpfn'],
        { encoding: 'utf8', timeout: 2000, maxBuffer: 65536,
          stdio: ['ignore', 'pipe', 'ignore'] });
      stage='listener-shape';
      const listeners = [];
      const listenerPids = new Set();
      let pid = 0;
      let descriptor = null;
      let shapeReason = null;
      for (const field of listing.trim().split('\n')) {
        if (/^p[0-9]+$/.test(field)) {
          pid = Number(field.slice(1));
          descriptor = null;
        } else if (/^f[0-9]+$/.test(field)) {
          descriptor = Number(field.slice(1));
        } else if (field.startsWith('n') && pid > 1 && descriptor !== null) {
          listeners.push({ pid, endpoint: field.slice(1) });
          listenerPids.add(pid);
          descriptor = null;
        } else {
          shapeReason ??= field.startsWith('f') ? 'malformed-descriptor'
            : field.startsWith('n') && pid > 1 ? 'missing-descriptor' : 'unexpected-field';
        }
      }
      if (shapeReason || listeners.length !== 1
          || listeners[0].endpoint !== `127.0.0.1:${port}`) {
        unixFailure = shapeReason ? (listing.trim() ? 'listener-shape' : 'listener-unavailable')
          : listeners.length === 0 ? 'listener-unavailable' : 'listener-shape';
        if (unixFailure === 'listener-shape') {
          listenerShape = {
            reason: shapeReason ?? (listeners.length > 1 ? 'multiple-listeners' : 'endpoint-mismatch'),
            listenerCount: listeners.length <= 4096 ? listeners.length : null,
            uniquePidCount: listenerPids.size <= 4096 ? listenerPids.size : null,
          };
        }
        return false;
      }
      stage='ancestor-query';
      const result=descendant(listeners[0].pid);
      if(!result&&unixFailure==='ancestor-unowned')unixFailure='listener-unowned';
      return result;
    } catch { unixFailure=stage;return false; }
  }
  // Associate the LISTEN socket inode with a child of the nanh launcher.
  try {
  const hexPort = Number(port).toString(16).toUpperCase().padStart(4, '0');
  const sockets = fs.readFileSync('/proc/net/tcp', 'utf8').trim().split('\n').slice(1)
    .map(line => line.trim().split(/\s+/))
    .filter(row => row[1] === `0100007F:${hexPort}` && row[3] === '0A');
  if(sockets.length!==1){unixFailure=sockets.length===0?'listener-unavailable':'listener-shape';return false;}
  const inode = `socket:[${sockets[0][9]}]`;
  for (const pid of fs.readdirSync('/proc').filter(x => /^\d+$/.test(x))) {
    try {
      if (!descendant(Number(pid))) continue;
      for (const fd of fs.readdirSync(`/proc/${pid}/fd`)) {
        if (fs.readlinkSync(`/proc/${pid}/fd/${fd}`) === inode) return true;
      }
    } catch { /* Processes can exit between enumeration and inspection. */ }
  }
  unixFailure='listener-unowned';
  return false;
  } catch(error) {unixFailure='listener-query';throw error;}
}
return { ownedEndpoint, descendant, parentPid, windowsProof, failure, failureDetails };
};
