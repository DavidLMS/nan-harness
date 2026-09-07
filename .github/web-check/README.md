# Website contract checks

Run from the repository root with the Node version in
`.github/web-check/.node-version`:

```sh
npm ci --prefix .github/web-check --ignore-scripts --no-audit --no-fund
node .github/web-check/node_modules/playwright/cli.js install chromium
node --check .github/scripts/check-web.mjs
node --check .github/scripts/check-web-browser.mjs
node .github/scripts/check-web.mjs
node .github/scripts/check-web-browser.mjs
```

On Linux, use `install --with-deps chromium` to install browser system
dependencies as well. The lockfile pins Playwright and its Chromium revision;
the installer reuses matching cached browser downloads. Update the exact Node
pin and package engine together, and regenerate the lockfile when updating
Playwright. No browser dependency is downloaded by the check itself, and a
missing module or executable fails the check.

The browser check is optional locally and required in the dedicated Web checks
workflow. It serves temporary staged assets on loopback, blocks external
origins, uses disposable browser contexts and a temporary browser profile,
and cleans up after success or failure. `PLAYWRIGHT_MODULE` can override the
default module with an absolute `index.mjs` path for local use. Optional
`WEB_SCREENSHOT_DIR` output contains only the synthetic local pages; CI does
not upload artifacts or receive deployment permissions.
