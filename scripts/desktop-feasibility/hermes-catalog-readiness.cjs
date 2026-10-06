'use strict';
// Passive wire evidence only: never send RPC, intercept traffic or retain frames.
exports.monitor = function monitor(session, guard, expectedModel, expectedProfile) {
  if (expectedModel !== 'qwen3.6' || typeof expectedProfile !== 'string'
      || !/^[A-Za-z0-9_-]{1,64}$/.test(expectedProfile)) throw new TypeError('Invalid readiness identity');
  let armed = false;
  let ambiguous = false;
  let socket = null;
  let verified = false;
  const pending = new Map();
  const parse = event => {
    const text = event.response?.payloadData;
    if (event.response?.opcode !== 1 || typeof text !== 'string' || text.length > 262144) return null;
    try { const value = JSON.parse(text); return value && !Array.isArray(value) && value.jsonrpc === '2.0' ? value : null; } catch { return null; }
  };
  const sent = event => {
    if (!guard()) { pending.clear(); verified = false; return; }
    const value = parse(event);
    if (!armed || !value || value.method !== 'model.options') return;
    if (!['number', 'string'].includes(typeof value.id) || value.params?.profile !== expectedProfile
        || value.params?.explicit_only !== true || (socket !== null && socket !== event.requestId)
        || pending.size !== 0 || verified) { ambiguous = true; verified = false; return; }
    socket = event.requestId;
    if (pending.size >= 32) { pending.clear(); return; }
    pending.set(value.id, socket);
  };
  const received = event => {
    if (!guard()) { pending.clear(); verified = false; return; }
    const value = parse(event);
    if (!value || event.requestId !== socket || pending.get(value.id) !== socket) return;
    pending.delete(value.id);
    const result = value.result;
    if (value.error || !result || !Array.isArray(result.providers) || result.providers.length > 4096) return;
    verified = !ambiguous && result.providers.filter(provider => provider && Array.isArray(provider.models)
      && provider.models.length <= 4096 && provider.models.includes(expectedModel)).length === 1;
  };
  const closed = event => { if (event.requestId === socket) { pending.clear(); verified = false; socket = null; ambiguous = true; } };
  session.on('Network.webSocketFrameSent', sent);
  session.on('Network.webSocketFrameReceived', received);
  session.on('Network.webSocketClosed', closed);
  return { arm() { armed = true; pending.clear(); socket = null; verified = false; ambiguous = false; },
    verified: () => Boolean(guard() && verified && !ambiguous), dispose() {
    session.off('Network.webSocketFrameSent', sent); session.off('Network.webSocketFrameReceived', received);
    session.off('Network.webSocketClosed', closed); pending.clear(); verified = false;
  } };
};
