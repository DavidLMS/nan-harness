import assert from "node:assert/strict";
const handlers = new Map();
const commands = new Map();
const entries = [];
const pi = {
  on(name, callback) { handlers.set(name, callback); },
  registerCommand(name, command) { commands.set(name, command); },
  appendEntry(customType, data) { entries.push({type: "custom", customType, data}); },
  setThinkingLevel() {},
};
const ctx = {
  model: {provider: "nan", id: "qwen3.6"},
  sessionManager: {getBranch: () => entries},
  ui: {setStatus() {}, notify() {}},
};
register(pi);
handlers.get("session_start")({}, ctx);
const request = (effort) => handlers.get("before_provider_request")({payload: {model: ctx.model.id, reasoning_effort: effort}}, ctx);
for (const effort of [undefined, "none", "high", "max"]) {
  assert.equal(request(effort), undefined, "Prime native intent must pass through without a custom override");
}
await commands.get("nan-reasoning").handler("auto", ctx);
assert.equal(request("none").reasoning_effort, undefined);
await commands.get("nan-reasoning").handler("off", ctx);
assert.equal(request(undefined).reasoning_effort, "none");
handlers.get("session_start")({}, ctx);
assert.equal(request(undefined).reasoning_effort, "none");
ctx.model = {provider: "nan", id: "deepseek-v4-flash"};
assert.equal(request("high").reasoning_effort, undefined, "Prime also omits unsupported controls");
