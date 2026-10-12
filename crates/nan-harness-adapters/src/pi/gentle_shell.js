import * as piAi from "@earendil-works/pi-ai";
import { fileURLToPath } from "node:url";

// Gentle Shell forwards this versioned package contract to every owned child.
// Append our extension after its packages so native provider registration wins.
function forwardGentleProvider() {
  const name = "GENTLE_SHELL_CHILD_PACKAGE_INJECTION";
  const value = process.env[name];
  const injection = value ? JSON.parse(value) : { version: 1, noExtensions: false, extensionPaths: [] };
  if (injection.version !== 1 || typeof injection.noExtensions !== "boolean" || !Array.isArray(injection.extensionPaths)) {
    throw new Error("Unsupported Gentle Shell child extension contract");
  }
  const path = fileURLToPath(import.meta.url);
  injection.extensionPaths = [...new Set([...injection.extensionPaths, path])];
  process.env[name] = JSON.stringify(injection);
}

function registerGentleNan(pi, config) {
  forwardGentleProvider();
  if (preserveGentleProvider) return;
  // Pi applies native registrations after legacy registrations, regardless of
  // CLI extension order. Use its native API for the competing Gentle provider.
  const api = piAi.lazyApi(async () => piAi.openAICompletionsApi());
  pi.registerProvider({
    id: "nan",
    name: "NaN",
    baseUrl: config.baseUrl,
    auth: { apiKey: {
      name: "NaN launch credential",
      async resolve() { return { auth: { apiKey: config.apiKey, baseUrl: config.baseUrl }, source: "nan-harness" }; }
    } },
    getModels: () => config.models.map(model => ({ ...model, provider: "nan", baseUrl: config.baseUrl, api: config.api })),
    // Native models.json overrides are composed after provider registration.
    // Fix routing at the request boundary as well, so a saved baseUrl/header
    // cannot send a launch token upstream or bypass the supervised gateway.
    stream: (model, context, options) => api.stream(
      { ...model, baseUrl: config.baseUrl, headers: undefined }, context,
      { ...options, apiKey: config.apiKey, headers: undefined }
    ),
    streamSimple: (model, context, options) => api.streamSimple(
      { ...model, baseUrl: config.baseUrl, headers: undefined }, context,
      { ...options, apiKey: config.apiKey, headers: undefined }
    )
  });
}
