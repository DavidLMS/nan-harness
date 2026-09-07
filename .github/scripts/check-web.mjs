import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { renderWeb, siteScripts } from './render-web.mjs';
import { prepareWeb } from './prepare-web.mjs';

// The three HTML pages load these classic scripts in this order before
// interactions.js; the renderer below runs them in a single shared context so
// app.js sees the locale content factories exactly as a browser does.
const orderedScripts = siteScripts();
const appSource = fs.readFileSync('web/app.js', 'utf8');
const styles = fs.readFileSync('web/styles.css', 'utf8');

function renderPage(page, locale = 'en', scripts = orderedScripts) {
  return renderWeb(page, locale, scripts).html;
}

const staging = fs.mkdtempSync(path.join(os.tmpdir(), 'nanh-web-'));
let robotsSource;
let sitemapSource;
let aiCatalogSource;
let skillsIndexSource;
let markdownFiles;
let skillFiles;
let publishedPaths;
try {
  prepareWeb(staging);
  publishedPaths = new Set(fs.readdirSync(staging, { recursive: true }));
  assertNoUnsupportedServices(staging);
  fs.writeFileSync(path.join(staging, 'auth.md'), 'synthetic unsupported service');
  assert.throws(() => assertNoUnsupportedServices(staging), /auth.md must not be published/);
  robotsSource = fs.readFileSync(path.join(staging, 'robots.txt'), 'utf8');
  sitemapSource = fs.readFileSync(path.join(staging, 'sitemap.xml'), 'utf8');
  aiCatalogSource = fs.readFileSync(path.join(staging, '.well-known', 'ai-catalog.json'), 'utf8');
  skillsIndexSource = fs.readFileSync(
    path.join(staging, '.well-known', 'agent-skills', 'index.json'),
    'utf8',
  );
  markdownFiles = new Map([
    'index.md',
    'docs.md',
    'logos.md',
    'es/index.md',
    'es/docs.md',
    'es/logos.md',
  ].map((relativePath) => [
    relativePath,
    fs.readFileSync(path.join(staging, relativePath), 'utf8'),
  ]));
  skillFiles = new Map(JSON.parse(skillsIndexSource).skills.map((skill) => [
    new URL(skill.url).pathname.replace('/nan-harness/', ''),
    fs.readFileSync(path.join(staging, new URL(skill.url).pathname.replace('/nan-harness/', '')), 'utf8'),
  ]));
  const bundle = ['app.js', fs.readFileSync(path.join(staging, 'app.js'), 'utf8')];
  for (const page of ['landing', 'docs', 'logos']) {
    for (const locale of ['en', 'es']) {
      const expected = renderPage(page, locale);
      // Both cached HTML and current HTML must work with the published app.js.
      assert.equal(renderPage(page, locale, [bundle]), expected);
      assert.equal(renderPage(page, locale, [...orderedScripts.slice(0, 2), bundle]), expected);
    }
  }
} finally {
  fs.rmSync(staging, { recursive: true, force: true });
}

assert.ok(fs.existsSync('web/.nojekyll'), 'Published static assets must not be filtered by Jekyll');
assert.match(robotsSource, /^User-agent: \*\nAllow: \/$/m);
assert.match(robotsSource, /^Content-Signal: search=yes, ai-input=yes$/m);
assert.doesNotMatch(robotsSource, /ai-train=/);
for (const crawler of ['OAI-SearchBot', 'Claude-Web', 'Google-Extended']) {
  assert.match(robotsSource, new RegExp(`^User-agent: ${crawler}\\nAllow: \\/$`, 'm'));
}
assert.match(robotsSource, /^Sitemap: https:\/\/davidlms\.github\.io\/nan-harness\/sitemap\.xml$/m);

for (const url of [
  'https://davidlms.github.io/nan-harness/',
  'https://davidlms.github.io/nan-harness/docs.html',
  'https://davidlms.github.io/nan-harness/logos.html',
]) {
  assert.match(sitemapSource, new RegExp(`<loc>${url}</loc>`));
}
assert.equal((sitemapSource.match(/<url>/g) ?? []).length, 3);
assert.match(sitemapSource, /<lastmod>\d{4}-\d{2}-\d{2}<\/lastmod>/);

const siteOrigin = 'https://davidlms.github.io/nan-harness';
for (const [file, canonicalPath] of [
  ['web/index.html', '/'],
  ['web/docs.html', '/docs.html'],
  ['web/logos.html', '/logos.html'],
]) {
  const source = fs.readFileSync(file, 'utf8');
  const slug = canonicalPath === '/' ? 'index' : canonicalPath.replace('/', '').replace('.html', '');
  assert.match(source, new RegExp(`<link rel="canonical" href="${siteOrigin}${canonicalPath}" />`));
  assert.match(
    source,
    new RegExp(`<link rel="alternate" type="text/markdown" hreflang="en" href="${siteOrigin}/${slug}.md" />`),
  );
  assert.match(
    source,
    new RegExp(`<link rel="alternate" type="text/markdown" hreflang="es" href="${siteOrigin}/es/${slug}.md" />`),
  );
}
assert.match(fs.readFileSync('web/index.html', 'utf8'), /rel="service-doc"/);
for (const file of ['web/index.html', 'web/docs.html', 'web/logos.html']) {
  assert.match(
    fs.readFileSync(file, 'utf8'),
    new RegExp(`<link rel="ai-catalog" type="application/ai-catalog\\+json" href="${siteOrigin}/\\.well-known/ai-catalog\\.json" />`),
  );
}

const aiCatalog = JSON.parse(aiCatalogSource);
assert.equal(aiCatalog.specVersion, '1.0');
assert.equal(aiCatalog.host.displayName, 'nan-harness');
assert.equal(aiCatalog.host.documentationUrl, `${siteOrigin}/docs.md`);
assert.ok(aiCatalog.entries.length >= 2);
for (const entry of aiCatalog.entries) {
  assert.match(entry.identifier, /^urn:air:davidlms\.github\.io:[^:]+:[^:]+$/);
  assert.ok(entry.displayName);
  assert.ok(['text/html', 'text/markdown', 'application/json'].includes(entry.type));
  assert.match(entry.url, /^https:\/\/davidlms\.github\.io\/nan-harness\//);
  assert.ok(Object.hasOwn(entry, 'url'));
  assert.ok(!Object.hasOwn(entry, 'data'));
  assert.ok(entry.description);
  assert.ok(publishedPaths.has(publishedPath(entry.url)), `Missing catalog target: ${entry.url}`);
}

const skillsIndex = JSON.parse(skillsIndexSource);
assert.equal(skillsIndex.$schema, 'https://schemas.agentskills.io/discovery/0.2.0/schema.json');
assert.ok(skillsIndex.skills.length >= 2);
for (const skill of skillsIndex.skills) {
  assert.match(skill.digest, /^sha256:[0-9a-f]{64}$/);
  assert.equal(skill.type, 'skill-md');
  assert.match(skill.url, /^https:\/\/davidlms\.github\.io\/nan-harness\//);
  const skillPath = new URL(skill.url).pathname.replace('/nan-harness/', '');
  const skillSource = skillFiles.get(skillPath);
  assert.ok(publishedPaths.has(skillPath), `Missing skill target: ${skill.url}`);
  assert.equal(
    `sha256:${crypto.createHash('sha256').update(skillSource).digest('hex')}`,
    skill.digest,
  );
  assert.ok(skillSource.startsWith(`---\nname: ${skill.name}\ndescription: ${skill.description}\n---\n`));
}

for (const relativePath of [
  'index.md',
  'docs.md',
  'logos.md',
  'es/index.md',
  'es/docs.md',
  'es/logos.md',
]) {
  const markdownSource = markdownFiles.get(relativePath);
  assert.doesNotMatch(markdownSource, /<(?:a|code|strong|em|br|span|p)\b|&lt;|&gt;/);
  assert.doesNotMatch(markdownSource, /\bundefined\b/);
  for (const [, href] of markdownSource.matchAll(/\]\(([^)]+)\)/g)) {
    const target = new URL(href);
    if (target.href.startsWith(`${siteOrigin}/`)) {
      assert.ok(publishedPaths.has(publishedPath(target.href)), `${relativePath}: missing ${href}`);
    }
  }
}
assert.match(markdownFiles.get('index.md'), /nanh codex --model qwen3\.6/);
assert.match(markdownFiles.get('docs.md'), /nanh hermes[\s\S]*nanh omp[\s\S]*nanh prime-agent/s);
assert.match(markdownFiles.get('es/docs.md'), /nanh hermes[\s\S]*nanh omp[\s\S]*nanh prime-agent/s);

function assertNoUnsupportedServices(directory) {
  for (const missingPath of [
    '.well-known/openid-configuration',
    '.well-known/oauth-authorization-server',
    '.well-known/oauth-protected-resource',
    '.well-known/mcp/server-card.json',
    'auth.md',
  ]) {
    assert.ok(!fs.existsSync(path.join(directory, missingPath)), `${missingPath} must not be published`);
  }
}

function publishedPath(url) {
  const relative = new URL(url).pathname.slice('/nan-harness/'.length);
  return relative || 'index.html';
}

const pagesWorkflow = fs.readFileSync('.github/workflows/pages.yml', 'utf8');
assert.match(pagesWorkflow, /path: \$\{\{ runner.temp \}\}\/nanh-web\n\s+include-hidden-files: true/);
assert.ok(publishedPaths.has('.nojekyll'));
for (const file of publishedPaths) {
  for (const [index, part] of file.split(path.sep).entries()) {
    if (!part.startsWith('.')) continue;
    assert.ok(index === 0 && ['.nojekyll', '.well-known'].includes(part), `Unexpected hidden asset: ${file}`);
  }
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
assert.match(landing, /class="hero-nan-lockup"><span>nanh<\/span>/);
assert.doesNotMatch(landing, /class="hero-nan-lockup"><span>nan<\/span>/);
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
for (const [path, source] of orderedScripts) {
  assert.doesNotMatch(source, /copy:\s*true/, `${path} must not enable harness copy mode`);
}
assert.doesNotMatch(styles, /@import\s/);
