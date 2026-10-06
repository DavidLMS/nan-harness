import assert from "node:assert/strict";
const handlers = new Map();
const commands = new Map();
const entries = [];
const notices = [];
const pi = {
  on(name, callback) { handlers.set(name, callback); },
  registerCommand(name, command) { commands.set(name, command); },
  appendEntry(customType, data) { entries.push({type: "custom", customType, data}); },
  setThinkingLevel() {},
};
const ctx = {
  model: {provider: "nan", id: "qwen3.6"},
  sessionManager: {getBranch: () => entries},
  ui: {setStatus() {}, notify(message) { notices.push(message); }},
};
register(pi);
const payload = (extra = {}) => handlers.get("before_provider_request")({payload: {model: ctx.model.id, reasoning_effort: "high", messages: [{role: "user", content: "Synthetic"}], ...extra}}, ctx);
const originalArgv = process.argv;
process.argv = [...originalArgv, "--thinking", "xhigh"];
handlers.get("session_start")({reason: "startup"}, ctx);
assert.equal(payload().reasoning_effort, "max", "startup CLI xhigh uses the supported maximum");
process.argv = [...originalArgv, "--thinking=xhigh"];
handlers.get("session_start")({reason: "startup"}, ctx);
assert.equal(payload().reasoning_effort, "max", "equals-form CLI alias uses the same maximum");
process.argv = originalArgv;
entries.length = 0;
handlers.get("session_start")({reason: "new"}, ctx);
assert.equal(payload().reasoning_effort, undefined, "native SDK default must not override auto");
await commands.get("nan-reasoning").handler("off", ctx);
assert.equal(payload().reasoning_effort, "none", "explicit off survives Pi collapse");
handlers.get("thinking_level_select")({level: "max"}, ctx);
assert.equal(payload().reasoning_effort, "max");
handlers.get("session_start")({reason: "resume"}, ctx);
assert.equal(payload().reasoning_effort, "max", "restore persisted per-model intent");
await commands.get("nan-reasoning").handler("auto", ctx);
assert.equal(payload().reasoning_effort, undefined);
handlers.get("session_tree")({}, ctx);
assert.equal(payload().reasoning_effort, undefined, "branch restoration retains auto");
ctx.model = {provider: "nan", id: "glm5.3-flash"};
handlers.get("thinking_level_select")({level: "low"}, ctx);
assert.equal(payload().reasoning_effort, undefined, "model switch clamping must not create intent");
handlers.get("model_select")({}, ctx);
await commands.get("nan-reasoning").handler("off", ctx);
assert.equal(notices.length, 1);
assert.equal(payload().reasoning_effort, undefined);
handlers.get("thinking_level_select")({level: "max"}, ctx);
assert.equal(payload().reasoning_effort, "max");
ctx.model = {provider: "nan", id: "deepseek-v4-flash"};
assert.equal(payload().reasoning_effort, undefined, "nonadjustable models omit effort");
ctx.model = {provider: "nan", id: "mimo-v2.6-flash"};
await commands.get("nan-reasoning").handler("off", ctx);
assert.deepEqual(payload({chat_template_kwargs: {preserve_thinking: true}}).chat_template_kwargs, {preserve_thinking: true, enable_thinking: false});
await commands.get("nan-reasoning").handler("auto", ctx);
assert.deepEqual(payload({chat_template_kwargs: {preserve_thinking: true, enable_thinking: false}}).chat_template_kwargs, {preserve_thinking: true}, "auto removes stale SDK toggles");
ctx.model = {provider: "other", id: "qwen3.6"};
assert.equal(payload(), undefined, "other providers remain untouched");
