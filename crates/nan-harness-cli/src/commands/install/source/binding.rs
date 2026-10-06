use std::io;

pub(super) fn entrypoint(source: &str) -> io::Result<String> {
    const START: &str = "void main();";
    const RUN: &str = "const exitCode = await run(context, {";
    if source.matches(START).count() != 1 || source.matches(RUN).count() != 1 {
        return Err(io::Error::other(
            "the pinned ZCode entrypoint contract changed",
        ));
    }
    // Use the upstream run API's explicit projectConfigPath while retaining its
    // own startup, terminal boundaries, telemetry shutdown and exit handling.
    // With no NaN configuration supplied, the normal ZCode entrypoint is unchanged.
    Ok(source
        .replace(
            START,
            r#"if (process.argv[2] === "--nanh-source-info") {
  console.log("nanh-zcode-config-v1");
} else {
  void main();
}"#,
        )
        .replace(
            RUN,
            r"const exitCode = await run(context, {
      projectConfigPath: process.env.NAN_HARNESS_ZCODE_PROJECT_CONFIG_FILE,",
        ))
}

pub(super) fn headless_entrypoint(source: &str) -> io::Result<String> {
    const APP_ENV: &str = "      env: appEnv,";
    if source.matches(APP_ENV).count() != 1 {
        return Err(io::Error::other(
            "the pinned ZCode headless configuration contract changed",
        ));
    }
    // The upstream TUI forwards projectConfigPath; its headless app omits it.
    // Forward the same public option so both entrypoints use the launch-owned file.
    Ok(source.replace(
        APP_ENV,
        "      env: appEnv,\n      projectConfigPath: deps.projectConfigPath,",
    ))
}
