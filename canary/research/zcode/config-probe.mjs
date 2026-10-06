import assert from "node:assert/strict";
import {
  decodeProviderConfigFile,
  encodeProviderConfigFile,
} from "zcode-source/packages/provider-node/src/provider-config-file-codec.ts";
const fixture = {
  schemaVersion: 1,
  config: {
    providerConfigRules: {
      providerRules: [
        {
          providerId: "nan",
          providerName: "NaN",
          enabled: true,
          config: {
            group: "standard-personal",
            access: { type: "api-key", apiKey: "synthetic-only" },
            api: { type: "openai-chat-completions", baseUrl: "http://127.0.0.1:12345/v1" },
            personalModelIds: ["synthetic-model"],
          },
        },
      ],
    },
    modelConfigRules: {
      providerModelRules: [{ providerId: "nan", modelId: "synthetic-model", config: {} }],
      manualProviderModelRules: [],
    },
    defaultModelSelection: { providerId: "nan", modelId: "synthetic-model" },
  },
};
const result = decodeProviderConfigFile(fixture);
assert.equal(result.defaultModelSelection.providerId, "nan");
assert.deepEqual(
  decodeProviderConfigFile(encodeProviderConfigFile(result)).defaultModelSelection,
  result.defaultModelSelection,
);
assert.throws(() => decodeProviderConfigFile({ ...fixture, schemaVersion: 2 }));
const bad = structuredClone(fixture);
bad.config.providerConfigRules.providerRules[0].config.access.apiKeyEnv = "NAN_API_KEY";
assert.throws(() => decodeProviderConfigFile(bad));
console.log(
  "PASS: current provider schema, encode/decode round trip, default model, future-schema rejection, unsupported environment-key reference rejection",
);
import { readFile } from "node:fs/promises";
import { ProviderConfigResolver } from "zcode-source/packages/provider/src/resolver.ts";
import {
  ProviderConfigMap,
  parseZCodeBuiltinProviderConfigRules,
  parseZCodeBuiltinModelConfigRules,
} from "zcode-source/packages/provider/src/config/index.ts";
import { readTuiSessionMetadata } from "zcode-source/apps/zcode-cli/packages/cli/src/tui-prompt-handler-queries.ts";
import { workflowActorModelPolicy } from "zcode-source/apps/zcode-cli/packages/bootstrap/src/app/workflow-actor-model.ts";
import { auxiliaryModelOptions } from "zcode-source/apps/zcode-cli/packages/core/src/model/auxiliary-model-options.ts";
const builtin = JSON.parse(
  await readFile(`${process.env.ZCODE_SOURCE_ROOT}/config/provider/zcode-builtin.json`, "utf8"),
).config;
const builtinProviders = parseZCodeBuiltinProviderConfigRules(builtin.providerConfigRules);
const resolverInput = {
  zcodeBuiltinProviders: builtinProviders.providers,
  zcodeBuiltinProviderTemplates: builtinProviders.providerTemplates,
  personalProviders: result.providers,
  zcodeBuiltinModelRules: parseZCodeBuiltinModelConfigRules(builtin.modelConfigRules),
  personalModels: result.models,
  accountProviders: new ProviderConfigMap(),
};
const resolution = new ProviderConfigResolver().resolve(resolverInput);
const nan = resolution.registryProviders.find((p) => p.providerId === "nan");
assert.ok(nan, "custom provider admitted without Z.ai account");
assert.ok(
  nan.models.some((m) => m.modelId === "synthetic-model"),
  "unknown model admitted",
);
assert.equal(
  (await readTuiSessionMetadata({ listModels: async () => [{ id: "nan/synthetic-model" }] }))
    .loginRequired,
  false,
);
assert.equal((await readTuiSessionMetadata({ listModels: async () => [] })).loginRequired, true);
assert.deepEqual(
  workflowActorModelPolicy({ parentSelection: result.defaultModelSelection }).configOverrides,
  {},
);
assert.equal(
  workflowActorModelPolicy(
    { parentSelection: result.defaultModelSelection },
    "other/previous-model",
  ).configOverrides.modelSelection.providerId,
  "other",
);
assert.deepEqual(
  auxiliaryModelOptions({
    optionSpecs: { reasoningLevel: { values: ["none", "high"] }, maxOutputTokens: { max: 2048 } },
  }),
  { reasoningLevel: "none", maxOutputTokens: 2048 },
);
console.log(
  "PASS: no-account custom-provider registry admission, unknown model admission, TUI gate with/without models, default actor inheritance, resume pin retention, auxiliary output/reasoning policy",
);

const restricted = new ProviderConfigResolver().resolve({
  ...resolverInput,
  zcodeBuiltinProviders: new ProviderConfigMap(),
  personalProviders: result.providers,
});
assert.deepEqual(
  restricted.registryProviders.map((provider) => provider.providerId),
  ["nan"],
);
console.log("PASS: explicit built-in provider isolation leaves only NaN selectable");
