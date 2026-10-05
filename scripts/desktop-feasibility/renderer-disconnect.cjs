// The inventory owns only its CDP client. The parent retains application and
// profile custody, rechecks them after this process exits, and owns cleanup.
exports.disconnect = async function(browser, facts, save, runtime = process) {
  facts.observerShutdown = 'disconnecting'; save();
  const timer = setTimeout(() => {
    facts.observerShutdown = 'disconnect-timeout'; save();
    // A completed inventory has no outstanding UI action. Exiting releases
    // this client's sockets without terminating the independently owned app.
    runtime.exit(facts.observerStage === 'complete' && facts.errorCategory === null ? 0 : 1);
  }, 2000);
  try {
    await browser.close();
    facts.observerShutdown = 'disconnected'; save();
  } finally { clearTimeout(timer); }
};
