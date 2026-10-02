// Bound native ancestry and listener ownership; no application data is read.
exports.proof = function(owner, port) {
const fs = require("node:fs");
function windowsProof(mode, value, root) {
  if (!['endpoint', 'descendant'].includes(mode) || !Number.isSafeInteger(value)
      || value <= 1 || value > (mode === 'endpoint' ? 65535 : 2147483647)
      || !Number.isSafeInteger(root) || root <= 1 || root > 2147483647) return false;
  try {
    const result = require('node:child_process').execFileSync('pwsh',
      ['-NoProfile', '-NonInteractive', '-File', `${__dirname}/endpoint-owner.ps1`,
        mode, String(value), String(root)],
      { encoding: 'utf8', timeout: 4000, maxBuffer: 4096,
        stdio: ['ignore', 'pipe', 'ignore'] });
    return result === 'true';
  } catch { return false; }
}
function descendant(pid) {
  if (process.platform === 'win32') return windowsProof('descendant', pid, Number(owner));
  for (let depth = 0; depth < 32 && pid > 1; depth++) {
    if (String(pid) === owner) return true;
    pid = parentPid(pid);
  }
  return false;
}
function parentPid(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 1) return 0;
  if (process.platform === 'darwin') {
    const parent = require('node:child_process').execFileSync('/bin/ps',
      ['-o', 'ppid=', '-p', String(pid)], { encoding: 'utf8', timeout: 2000,
        maxBuffer: 4096, stdio: ['ignore', 'pipe', 'ignore'] }).trim();
    return /^[0-9]+$/.test(parent) ? Number(parent) : 0;
  }
  const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
  return Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]);
}
function ownedEndpoint() {
  if (process.platform === 'win32') return windowsProof('endpoint', Number(port), Number(owner));
  if (process.platform === 'darwin') {
    // lsof selects listeners by port; reject wildcard/non-loopback bindings.
    try {
      const listing = require('node:child_process').execFileSync('/usr/sbin/lsof',
        ['-nP', '-a', `-iTCP:${port}`, '-sTCP:LISTEN', '-Fpn'],
        { encoding: 'utf8', timeout: 2000, maxBuffer: 65536,
          stdio: ['ignore', 'pipe', 'ignore'] });
      const listeners = [];
      let pid = 0;
      for (const field of listing.trim().split('\n')) {
        if (/^p[0-9]+$/.test(field)) pid = Number(field.slice(1));
        else if (field.startsWith('n')) listeners.push({ pid, endpoint: field.slice(1) });
        else return false;
      }
      return listeners.length === 1 && listeners[0].endpoint === `127.0.0.1:${port}`
        && descendant(listeners[0].pid);
    } catch { return false; }
  }
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
return { ownedEndpoint, descendant, parentPid, windowsProof };
};
