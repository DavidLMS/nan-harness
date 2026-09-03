import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

const appSource = fs.readFileSync('web/app.js', 'utf8');
const styles = fs.readFileSync('web/styles.css', 'utf8');

function renderPage(page, locale = 'en') {
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
    localStorage: { getItem: (key) => key === 'nan-harness-locale' ? locale : null },
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
const logos = renderPage('logos');
const landingEs = renderPage('landing', 'es');
const docsEs = renderPage('docs', 'es');
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

function assertUniqueIds(html, page) {
  const ids = [...html.matchAll(/\sid="([^"]+)"/g)].map((match) => match[1]);
  assert.equal(new Set(ids).size, ids.length, `${page} must not contain duplicate IDs`);
}

function assertFragmentTargets(html, page) {
  for (const [, fragment] of html.matchAll(/href="#([^"]+)"/g)) {
    assert.match(html, new RegExp(`\\sid="${fragment}"`), `${page} must render the #${fragment} target`);
  }
}

for (const [page, html] of [
  ['landing', landing],
  ['docs', docs],
  ['logos', logos],
  ['landing (es)', landingEs],
  ['docs (es)', docsEs],
]) {
  assert.match(html, /class="skip-link"/);
  assert.match(html, /id="main-content"/);
  assertUniqueIds(html, page);
  assertFragmentTargets(html, page);
  for (const anchor of html.match(/<a\b[^>]*target="_blank"[^>]*>/g) ?? []) {
    assert.match(anchor, /rel="noreferrer"/, `${page} external links must protect window.opener`);
  }
}

assert.ok(autoplayInitialDelay > 0 && autoplayInitialDelay < autoplayInterval);
assert.equal((landing.match(/role="option"/g) ?? []).length, harnessIds.length);
assert.equal((landing.match(/role="listbox"/g) ?? []).length, 1);
assert.equal((landing.match(/data-picker-item(?:\s|>)/g) ?? []).length, harnessIds.length * 5);
assert.match(landing, /aria-activedescendant="picker-option-claude"/);
assert.match(landing, /data-picker-track aria-hidden="true"/);
assert.match(landing, /data-picker-autoplay data-state="playing"/);
assert.match(landing, /href="logos\.html"/);
assert.match(landing, /<section class="hero page-width">/);
assert.match(landing, /class="hero-lede"/);
assert.match(landing, /class="section-space community-section/);
assert.match(landing, /class="section-space feature-section/);
assert.match(landing, /class="section-space feature-section telemetry-section/);
assert.match(landing, /class="section-space faq-section page-width" id="faq"/);
assert.match(landing, /<section class="final-cta">/);
assert.match(landing, /nanh opencode.*nanh config opencode.*opencode/s);
assert.match(landing, /~\/nanh\/harness/);
assert.doesNotMatch(landing, /~\/nan\/harness/);
assert.match(docs, /class="docs-sidebar" aria-label=/);
assert.match(docs, /class="docs-breadcrumb" aria-label=/);
assert.match(docs, /<code>nanh &lt;harness&gt;<\/code>/);
assert.match(docs, /<code>nanh config &lt;harness&gt;<\/code>/);
assert.match(docs, /nanh hermes.*nanh omp.*nanh prime-agent/s);

const docsSectionIds = ['install', 'first-run', 'harnesses', 'desktop', 'search', 'options', 'help'];
for (const html of [docs, docsEs]) {
  for (const sectionId of docsSectionIds) {
    assert.equal((html.match(new RegExp(`<section class="docs-section" id="${sectionId}">`, 'g')) ?? []).length, 1);
    assert.match(html, new RegExp(`href="#${sectionId}"`));
  }
}

const faqCount = (landing.match(/<details class="faq-row">/g) ?? []).length;
assert.ok(faqCount > 0);
assert.equal((landing.match(/<summary>/g) ?? []).length, faqCount);
assert.equal((landingEs.match(/<details class="faq-row">/g) ?? []).length, faqCount);
assert.match(landing, /data-locale="en" aria-pressed="true"/);
assert.match(landingEs, /data-locale="es" aria-pressed="true"/);
assert.match(logos, /class="docs-main logos-main"/);
assert.equal((logos.match(/<section class="logos-section">/g) ?? []).length, 2);

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
