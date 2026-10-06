function nanThinkingLevels(policy) {
  const levels = Object.fromEntries(["off", "minimal", "low", "medium", "high", "xhigh", "max"].map(level => [level, null]));
  if (policy.kind === "effort") {
    for (const level of policy.supported) levels[level] = level;
    if (policy.supportsDisabled) levels.off = "none";
  } else if (policy.kind === "toggle") {
    levels.off = "none";
    levels.high = "high";
  }
  return levels;
}

function registerNanReasoning(pi, profiles, preserveNativeDefaults = false) {
  const entryType = "nan-reasoning";
  const intents = new Map();
  let activeModel;
  const policyFor = model => model?.provider === "nan" ? profiles[model.id]?.reasoningPolicy : undefined;
  const display = ctx => {
    if (policyFor(ctx.model)) ctx.ui.setStatus(entryType, `NaN reasoning: ${intents.get(ctx.model.id) ?? "auto"}`);
    else ctx.ui.setStatus(entryType, undefined);
  };
  const save = (level, ctx) => {
    if (!policyFor(ctx.model)) return;
    intents.set(ctx.model.id, level);
    pi.appendEntry(entryType, { model: ctx.model.id, level });
    display(ctx);
  };
  const restore = (_event, ctx) => {
    intents.clear();
    activeModel = ctx.model?.id;
    for (const entry of ctx.sessionManager.getBranch()) {
      if (entry.type === "custom" && entry.customType === entryType && profiles[entry.data?.model]) {
        intents.set(entry.data.model, entry.data.level);
      }
    }
    display(ctx);
  };
  pi.on("session_start", (event, ctx) => {
    restore(event, ctx);
    // CLI selection precedes extension events; preserve it before Pi collapses off.
    if (event.reason === "startup") {
      const index = process.argv.indexOf("--thinking");
      const argument = process.argv.find(value => value.startsWith("--thinking="));
      const level = argument?.slice("--thinking=".length) ?? (index >= 0 ? process.argv[index + 1] : undefined);
      if (level) {
        const supported = nanThinkingLevels(policyFor(ctx.model) ?? {});
        // Native selectors emit their clamped level; CLI aliases arrive before those events.
        const normalized = level === "xhigh" ? (supported.max ? "max" : "high") : level;
        save(normalized, ctx);
      }
    }
  });
  pi.on("session_tree", restore);
  pi.on("model_select", (_event, ctx) => {
    activeModel = ctx.model?.id;
    display(ctx);
  });
  pi.on("thinking_level_select", (event, ctx) => {
    // Model switches clamp native levels before model_select; that is not user intent.
    if (activeModel === ctx.model?.id) save(event.level, ctx);
  });
  pi.registerCommand("nan-reasoning", {
    description: "Select NaN reasoning: auto, off, low, medium, high, max",
    async handler(args, ctx) {
      const policy = policyFor(ctx.model);
      if (!policy) return;
      const level = args.trim() || "auto";
      const supported = nanThinkingLevels(policy);
      if (level !== "auto" && !supported[level]) {
        ctx.ui.notify("This reasoning level is not supported by the selected NaN model.", "warning");
        return;
      }
      if (level !== "auto") pi.setThinkingLevel(level);
      save(level, ctx);
    }
  });
  pi.on("before_provider_request", (event, ctx) => {
    if (!policyFor(ctx.model) || !event.payload || typeof event.payload !== "object") return;
    const policy = profiles[event.payload.model]?.reasoningPolicy;
    if (!policy) return;
    // Prime retains native omission versus off; only explicit NaN intent overrides it.
    if (preserveNativeDefaults && !intents.has(event.payload.model) && policy.kind === "effort") return;
    const payload = { ...event.payload };
    delete payload.reasoning_effort;
    delete payload.enable_thinking;
    delete payload.thinking;
    if (payload.chat_template_kwargs && typeof payload.chat_template_kwargs === "object") {
      payload.chat_template_kwargs = { ...payload.chat_template_kwargs };
      delete payload.chat_template_kwargs.enable_thinking;
      if (Object.keys(payload.chat_template_kwargs).length === 0) delete payload.chat_template_kwargs;
    }
    const level = intents.get(event.payload.model) ?? "auto";
    if (policy.kind === "effort") {
      if (level === "off" && policy.supportsDisabled) payload.reasoning_effort = "none";
      else if (policy.supported.includes(level)) payload.reasoning_effort = level;
    } else if (policy.kind === "toggle" && level !== "auto") {
      payload.chat_template_kwargs = { ...payload.chat_template_kwargs, enable_thinking: level !== "off" };
    }
    return payload;
  });
}
