import assert from "node:assert/strict";
import http from "node:http";
import { once } from "node:events";
import { streamText, generateText, tool, jsonSchema, stepCountIs } from "ai";
import { AiSdkModelExecution } from "zcode-source/apps/zcode-cli/packages/adapters/src/model/model-execution.ts";
let mode = "stream",
  requests = [],
  calls = 0;
const server = http.createServer(async (req, res) => {
  let raw = "";
  for await (const chunk of req) raw += chunk;
  const body = JSON.parse(raw);
  requests.push({ path: req.url, auth: req.headers.authorization, body });
  if (mode === "error") {
    res.writeHead(401, { "content-type": "application/json" });
    res.end(
      JSON.stringify({
        error: { message: "synthetic auth failure", type: "authentication_error" },
      }),
    );
    return;
  }
  if (mode === "cancel") {
    res.writeHead(200, { "content-type": "text/event-stream" });
    res.write(": waiting\n\n");
    return;
  }
  const toolTurn = mode === "tool" && calls++ === 0;
  const choice = toolTurn
    ? {
        index: 0,
        delta: {
          tool_calls: [
            {
              index: 0,
              id: "probe-call",
              type: "function",
              function: { name: "probe_tool", arguments: '{"value":"synthetic"}' },
            },
          ],
        },
        finish_reason: "tool_calls",
      }
    : { index: 0, delta: { content: "PROBE_OK" }, finish_reason: "stop" };
  res.writeHead(200, { "content-type": "text/event-stream" });
  res.end(
    `data: ${JSON.stringify({ id: "probe", object: "chat.completion.chunk", created: 1, model: body.model, choices: [choice] })}\n\ndata: ${JSON.stringify({ id: "probe", object: "chat.completion.chunk", created: 1, model: body.model, choices: [], usage: { prompt_tokens: 12, completion_tokens: 4, total_tokens: 16 } })}\n\ndata: [DONE]\n\n`,
  );
});
server.listen(0, "127.0.0.1");
await once(server, "listening");
const baseUrl = `http://127.0.0.1:${server.address().port}/v1`;
const execution = new AiSdkModelExecution({ env: {} });
const bound = execution.bindModel({
  providerId: "nan",
  modelId: "synthetic-model",
  supportsJsonSchemaOutput: false,
  providerConfig: {
    access: { type: "api-key", apiKey: "synthetic-only" },
    api: { type: "openai-chat-completions", baseUrl },
  },
  optionSpecs: { reasoningLevel: { map: "{}" }, maxOutputTokens: { map: "{}" } },
});
try {
  const response = streamText({
    model: bound.resolved.model,
    prompt: "synthetic probe",
    maxRetries: 0,
  });
  assert.equal(await response.text, "PROBE_OK");
  assert.equal((await response.usage).inputTokens, 12);
  assert.equal(requests[0].path, "/v1/chat/completions");
  assert.equal(requests[0].auth, "Bearer synthetic-only");
  assert.equal(requests[0].body.model, "synthetic-model");
  assert.equal(requests[0].body.stream_options.include_usage, true);
  mode = "tool";
  let executed = false;
  const result = streamText({
    model: bound.resolved.model,
    prompt: "synthetic tool probe",
    maxRetries: 0,
    stopWhen: stepCountIs(3),
    tools: {
      probe_tool: tool({
        description: "Synthetic test tool",
        inputSchema: jsonSchema({
          type: "object",
          properties: { value: { type: "string" } },
          required: ["value"],
          additionalProperties: false,
        }),
        execute: async ({ value }) => {
          assert.equal(value, "synthetic");
          executed = true;
          return "synthetic result";
        },
      }),
    },
  });
  assert.equal(await result.text, "PROBE_OK");
  assert.equal(executed, true);
  assert.ok(requests.at(-1).body.messages.some((m) => m.role === "tool"));
  mode = "error";
  await assert.rejects(
    generateText({ model: bound.resolved.model, prompt: "synthetic failure probe", maxRetries: 0 }),
    (e) => e.statusCode === 401,
  );
  mode = "cancel";
  await assert.rejects(
    generateText({
      model: bound.resolved.model,
      prompt: "synthetic cancellation probe",
      maxRetries: 0,
      abortSignal: AbortSignal.timeout(200),
    }),
  );
  console.log(
    "PASS: upstream model factory, direct authenticated Chat Completions, selected model, SSE termination, usage, tool execution/result continuation, HTTP 401, cancellation",
  );
} finally {
  server.closeAllConnections();
  server.close();
}
