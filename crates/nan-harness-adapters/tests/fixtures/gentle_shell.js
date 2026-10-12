import assert from "node:assert/strict";
process.env.NAN_API_KEY = "synthetic-launch-token";
process.env.GENTLE_SHELL_CHILD_PACKAGE_INJECTION = JSON.stringify({version: 1, noExtensions: true, extensionPaths: ["/gentle/package"]});
let registered;
const handlers = new Map();
let tools = [{name: "web_search", owner: "existing"}];
const pi = {
  registerProvider(provider) { registered = provider; },
  on(event, callback) { handlers.set(event, callback); },
  registerCommand() {},
  getAllTools() { return tools; },
  registerTool(tool) { tools = [tool]; }
};
registerNan(pi);
const injection = JSON.parse(process.env.GENTLE_SHELL_CHILD_PACKAGE_INJECTION);
assert.equal(injection.noExtensions, true);
assert.equal(injection.extensionPaths[0], "/gentle/package");
assert.equal(injection.extensionPaths.length, 2);
assert.equal(Boolean(registered), !preserveGentleProvider);
if (registered) {
  for (const method of ["stream", "streamSimple"]) {
    registered[method]({baseUrl: "https://saved.invalid", headers: {authorization: "saved"}}, {}, {apiKey: "saved", headers: {authorization: "saved"}});
    assert.equal(calls.at(-1)[0].baseUrl, registered.baseUrl);
    assert.equal(calls.at(-1)[0].headers, undefined);
    assert.equal(calls.at(-1)[2].apiKey, "synthetic-launch-token");
    assert.equal(calls.at(-1)[2].headers, undefined);
  }
  assert.equal(registered.id, "nan");
  assert.equal((await registered.auth.apiKey.resolve({credential: {key: "stored-secret"}})).auth.apiKey, "synthetic-launch-token");
}
handlers.get("resources_discover")();
const forced = !tools[0].owner;
assert.equal(forced, expectedForce);
if (!forced) {
  tools = [];
  handlers.get("resources_discover")();
  assert.equal(tools[0].name, "web_search");
}
registerNan(pi);
assert.equal(JSON.parse(process.env.GENTLE_SHELL_CHILD_PACKAGE_INJECTION).extensionPaths.length, 2);
process.env.GENTLE_SHELL_CHILD_PACKAGE_INJECTION = '{"version":2}';
assert.throws(() => registerNan(pi), /Unsupported Gentle Shell/);
