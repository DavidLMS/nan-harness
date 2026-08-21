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

assert.equal((landing.match(/role="option"/g) ?? []).length, harnessIds.length);
assert.equal((landing.match(/role="listbox"/g) ?? []).length, 1);
assert.match(landing, /aria-activedescendant="picker-option-claude"/);
assert.match(landing, /class="skip-link"/);
assert.match(landing, /id="main-content"/);
assert.match(docs, /class="docs-sidebar" aria-label=/);
assert.match(docs, /class="docs-breadcrumb" aria-label=/);
assert.match(docs, /id="main-content"/);

for (const harnessId of harnessIds) {
  assert.equal((landing.match(new RegExp(`id="picker-option-${harnessId}"`, 'g')) ?? []).length, 1);
}

assert.doesNotMatch(appSource, /logos\//);
assert.doesNotMatch(appSource, /copy:\s*true/);
assert.doesNotMatch(styles, /@import\s/);
