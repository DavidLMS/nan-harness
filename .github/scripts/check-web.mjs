import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

const appSource = fs.readFileSync('web/app.js', 'utf8');
const styles = fs.readFileSync('web/styles.css', 'utf8');
const readme = fs.readFileSync('README.md', 'utf8');

function renderPage(page) {
  const app = { innerHTML: '' };
  const meta = { content: '' };
  const document = {
    body: { className: '', dataset: { page } },
    documentElement: { lang: '' },
    title: '',
    getElementById: (id) => (id === 'app' ? app : null),
    querySelector: (selector) => (selector === 'meta[name="description"]' ? meta : null),
    addEventListener: () => {},
  };
  const window = {
    localStorage: { getItem: () => null },
  };
  const navigator = {
    language: 'en',
    languages: ['en'],
    userAgent: 'test',
  };

  vm.runInNewContext(appSource, { document, navigator, window });
  return app.innerHTML;
}

const landing = renderPage('landing');
const docs = renderPage('docs');
const autoplayInitialDelay = Number(appSource.match(/const AUTOPLAY_INITIAL_DELAY_MS = (\d+);/)?.[1]);
const autoplayInterval = Number(appSource.match(/const AUTOPLAY_INTERVAL_MS = (\d+);/)?.[1]);
const harnessIds = [
  'claude',
  'codex',
  'opencode',
  'hermes',
  'omp',
  'pi',
  'prime',
  'deepseek',
  'openclaw',
  'cline',
  'qwen',
  'kimi',
  'aider',
  'goose',
  'fx',
];
const logoHarnessIds = harnessIds.filter((harnessId) => harnessId !== 'fx');
const logoFiles = Object.fromEntries(logoHarnessIds.map((harnessId) => [harnessId, `${harnessId}.svg`]));
logoFiles.codex = 'codex.png';
logoFiles.hermes = 'hermes.png';

assert.ok(autoplayInitialDelay > 0 && autoplayInitialDelay < autoplayInterval);
assert.equal((landing.match(/role="option"/g) ?? []).length, harnessIds.length);
assert.equal((landing.match(/role="listbox"/g) ?? []).length, 1);
assert.equal((landing.match(/data-picker-item(?:\s|>)/g) ?? []).length, harnessIds.length * 5);
assert.match(landing, /aria-activedescendant="picker-option-claude"/);
assert.match(landing, /data-picker-track aria-hidden="true"/);
assert.match(landing, /data-picker-autoplay data-state="playing"/);
assert.match(landing, /href="logos\.html"/);
assert.match(landing, /class="skip-link"/);
assert.match(landing, /id="main-content"/);
assert.match(landing, /managed NaN route.*works across every supported harness.*optional error reporting/i);
assert.match(landing, /THE RECOMMENDED WAY/);
assert.match(landing, /recommended: checks, routes and supervises OpenCode/i);
assert.match(landing, /advanced: writes persistent OpenCode configuration/i);
assert.match(landing, /nan opencode.*nan config opencode.*opencode/s);
assert.match(docs, /class="docs-sidebar" aria-label=/);
assert.match(docs, /class="docs-breadcrumb" aria-label=/);
assert.match(docs, /id="main-content"/);
assert.match(docs, /nan-harness.*full command.*nan.*shorter alias/i);
assert.match(docs, /recommended workflow for every supported agent.*advanced option/i);
assert.match(docs, /Claude Code, Codex and fx need nan-harness running/i);
assert.match(docs, /nan hermes.*nan omp.*nan prime-agent/s);
assert.match(readme, /nan hermes.*nan omp.*nan prime-agent/s);
assert.match(landing, /When should I use nan config\?/i);
assert.doesNotMatch(landing, /Do I have to start agents with nan(?:-harness)?\?/i);
assert.match(appSource, /¿Cuándo debería usar nan config\?/i);
assert.doesNotMatch(appSource, /¿Tengo que arrancar los agentes con nan(?:-harness)?\?/i);
assert.doesNotMatch(docs, /launch commands are temporary/i);

for (const harnessId of harnessIds) {
  assert.equal((landing.match(new RegExp(`id="picker-option-${harnessId}"`, 'g')) ?? []).length, 1);
}

for (const [harnessId, logoFile] of Object.entries(logoFiles)) {
  const logoPath = `web/logos/${logoFile}`;
  const logoSource = fs.readFileSync(logoPath);
  assert.ok(landing.split(`logos/${logoFile}`).length - 1 >= 5, `${harnessId} logo must be rendered`);
  if (logoFile.endsWith('.svg')) {
    assert.doesNotMatch(logoSource.toString('utf8'), /<script|<foreignObject|\son[a-z]+\s*=|(?:href|xlink:href)=["']https?:/i);
  } else {
    assert.equal(logoSource.subarray(0, 8).toString('hex'), '89504e470d0a1a0a', `${logoFile} must be a PNG`);
  }
}

const ompLogo = fs.readFileSync('web/logos/omp.svg', 'utf8');
assert.match(ompLogo, /viewBox="0 0 64 64"/);
assert.match(ompLogo, /M10 14h44v9H43v33h-9V23h-9v22h-9V23H10z/);
assert.match(ompLogo, /oklch\(0\.7 0\.24 340\).*oklch\(0\.62 0\.21 295\).*oklch\(0\.81 0\.14 200\)/s);

for (const noticePath of [
  'web/logos/README.md',
  'web/logos/licenses/APACHE-2.0.txt',
  'web/logos/licenses/CC0-1.0.txt',
  'web/logos/licenses/SIMPLE-ICONS-DISCLAIMER.md',
  'web/logos/licenses/MIT-hermes-agent.txt',
  'web/logos/licenses/MIT-openclaw.txt',
  'web/logos/licenses/MIT-prime-agent.txt',
]) {
  assert.ok(fs.existsSync(noticePath), `${noticePath} must exist`);
}

assert.match(appSource, /IntersectionObserver/);
assert.match(appSource, /prefers-reduced-motion: reduce/);
assert.doesNotMatch(appSource, /copy:\s*true/);
assert.doesNotMatch(styles, /@import\s/);
