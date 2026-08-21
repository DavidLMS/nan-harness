import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

const appSource = fs.readFileSync('web/app.js', 'utf8');
const styles = fs.readFileSync('web/styles.css', 'utf8');

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
const harnessIds = [
  'claude',
  'codex',
  'opencode',
  'hermes',
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

assert.equal((landing.match(/role="option"/g) ?? []).length, harnessIds.length);
assert.equal((landing.match(/role="listbox"/g) ?? []).length, 1);
assert.equal((landing.match(/data-picker-item(?:\s|>)/g) ?? []).length, harnessIds.length * 5);
assert.match(landing, /aria-activedescendant="picker-option-claude"/);
assert.match(landing, /data-picker-track aria-hidden="true"/);
assert.match(landing, /data-picker-autoplay data-state="playing"/);
assert.match(landing, /href="logos\/README\.md"/);
assert.match(landing, /class="skip-link"/);
assert.match(landing, /id="main-content"/);
assert.match(docs, /class="docs-sidebar" aria-label=/);
assert.match(docs, /class="docs-breadcrumb" aria-label=/);
assert.match(docs, /id="main-content"/);

for (const harnessId of harnessIds) {
  assert.equal((landing.match(new RegExp(`id="picker-option-${harnessId}"`, 'g')) ?? []).length, 1);
}

for (const harnessId of logoHarnessIds) {
  const logoPath = `web/logos/${harnessId}.svg`;
  const logoSource = fs.readFileSync(logoPath, 'utf8');
  assert.ok((landing.match(new RegExp(`logos/${harnessId}\\.svg`, 'g')) ?? []).length >= 5);
  assert.doesNotMatch(logoSource, /<script|<foreignObject|\son[a-z]+\s*=|(?:href|xlink:href)=["']https?:/i);
}

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
