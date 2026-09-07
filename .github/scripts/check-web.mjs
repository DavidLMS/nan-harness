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
  assertPublishedHtml(staging);
  publishedPaths = new Set(fs.readdirSync(staging, { recursive: true }));
  assertNoLegacyOrigin(staging);
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
    publishedPath(skill.url),
    fs.readFileSync(path.join(staging, publishedPath(skill.url)), 'utf8'),
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
assert.match(robotsSource, /^Sitemap: https:\/\/nan-harness\.davidlms\.com\/sitemap\.xml$/m);

for (const url of [
  'https://nan-harness.davidlms.com/',
  'https://nan-harness.davidlms.com/docs.html',
  'https://nan-harness.davidlms.com/logos.html',
]) {
  assert.match(sitemapSource, new RegExp(`<loc>${url}</loc>`));
}
assert.equal((sitemapSource.match(/<url>/g) ?? []).length, 3);
assert.match(sitemapSource, /<lastmod>\d{4}-\d{2}-\d{2}<\/lastmod>/);

const siteOrigin = 'https://nan-harness.davidlms.com';
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
  assert.match(entry.identifier, /^urn:air:nan-harness\.davidlms\.com:[^:]+:[^:]+$/);
  assert.ok(entry.displayName);
  assert.ok(['text/html', 'text/markdown', 'application/json'].includes(entry.type));
  assert.match(entry.url, /^https:\/\/nan-harness\.davidlms\.com\//);
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
  assert.match(skill.url, /^https:\/\/nan-harness\.davidlms\.com\//);
  const skillPath = publishedPath(skill.url);
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

function assertNoLegacyOrigin(directory) {
  const legacyOrigin = 'https://davidlms.github.io/nan-harness';
  for (const relativePath of fs.readdirSync(directory, { recursive: true })) {
    const filePath = path.join(directory, relativePath);
    if (!fs.statSync(filePath).isFile()) continue;
    assert.doesNotMatch(fs.readFileSync(filePath, 'utf8'), new RegExp(legacyOrigin.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
      `${relativePath} must not publish the legacy site origin`);
  }
}

function publishedPath(url) {
  const relative = new URL(url).pathname.slice(1);
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

function assertPublishedHtml(directory) {
  const pages = new Map();
  for (const [file, page] of [['index.html', 'landing'], ['docs.html', 'docs'], ['logos.html', 'logos']]) {
    const html = fs.readFileSync(path.join(directory, file), 'utf8');
    const rendered = renderWeb(page);
    pages.set(file, html);
    assert.ok(html.includes(`<div id="app">${rendered.html}</div>`), `${file}: shared renderer content`);
    assert.ok(html.includes(`class="${rendered.bodyClass}"`), `${file}: initial body class`);
    assert.ok(html.includes(`<title>${rendered.title}</title>`), `${file}: initial title`);
    assert.ok(html.includes(`content="${rendered.description}"`), `${file}: initial description`);
    assert.match(html, /<html lang="en">/);
    assert.equal((html.match(/<main\b/g) ?? []).length, 1, `${file}: main landmark`);
    assert.equal((html.match(/<h1\b/g) ?? []).length, 1, `${file}: primary heading`);
    assert.match(html, /<h2\b/);
    assert.doesNotMatch(html, /data-enhanced/);
    assertUniqueIds(html, file);
    assertFragmentTargets(html, file);
  }
  assert.match(pages.get('index.html'), /class="install-fallback"[\s\S]*install\.sh[\s\S]*install\.ps1/);
  assert.match(pages.get('index.html'), /href="logos\.html"/);
  assert.match(pages.get('docs.html'), /id="install"[\s\S]*id="harnesses"[\s\S]*id="help"/);
  assert.match(pages.get('logos.html'), /href="logos\/licenses\/APACHE-2\.0\.txt"/);
  for (const [file, html] of pages) {
    for (const [, href] of html.matchAll(/href="([^"]+)"/g)) {
      const target = new URL(href, `https://nan-harness.davidlms.com/${file}`);
      if (target.origin !== 'https://nan-harness.davidlms.com') continue;
      const targetFile = publishedPath(target.href);
      assert.ok(fs.existsSync(path.join(directory, targetFile)), `${file}: missing ${href}`);
      if (target.hash && pages.has(targetFile)) {
        assert.ok(pages.get(targetFile).includes(`id="${target.hash.slice(1)}"`), `${file}: missing ${href}`);
      }
    }
  }
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

const webMcpRegistrations = [];
const webMcpEvents = [];
let openedDocumentationUrl;
const webMcpRender = renderWeb('docs', 'en', orderedScripts, {
  AbortController,
  modelContext: {
    registerTool(tool, options) {
      webMcpRegistrations.push({ tool, options });
      return Promise.resolve();
    },
  },
  window: {
    addEventListener(type, listener, options) {
      webMcpEvents.push({ type, listener, options });
    },
    location: {
      assign(url) {
        openedDocumentationUrl = url;
      },
    },
  },
});
assert.equal(webMcpRegistrations.length, 2);
assert.deepEqual(
  webMcpRegistrations.map(({ tool }) => tool.name).sort(),
  ['open_documentation', 'search_documentation'],
);
const webMcpTools = Object.fromEntries(webMcpRegistrations.map(({ tool }) => [tool.name, tool]));
for (const tool of Object.values(webMcpTools)) {
  assert.ok(tool.description);
  assert.equal(tool.annotations.readOnlyHint, tool.name === 'search_documentation');
  assert.deepEqual(tool.inputSchema.type, 'object');
  assert.deepEqual([...tool.inputSchema.required], [tool.name === 'search_documentation' ? 'query' : 'sectionId']);
  assert.equal(tool.inputSchema.additionalProperties, false);
}
assert.equal(webMcpTools.search_documentation.inputSchema.properties.query.minLength, 1);
assert.equal(webMcpTools.search_documentation.inputSchema.properties.query.maxLength, 200);
assert.deepEqual(
  [...webMcpTools.open_documentation.inputSchema.properties.sectionId.enum],
  docsSectionIds,
);

const titleQuery = webMcpRender.copy.docsSections[0][1].toUpperCase();
const titleMatches = webMcpTools.search_documentation.execute({ query: titleQuery });
assert.equal(titleMatches.results[0].sectionId, docsSectionIds[0]);
const contentMatches = webMcpTools.search_documentation.execute({ query: 'NaNh CoNfIg' });
assert.ok(contentMatches.results.some(({ sectionId }) => sectionId === 'harnesses'));
assert.ok(contentMatches.results.length <= 5);
for (const result of contentMatches.results) {
  assert.deepEqual(Object.keys(result).sort(), ['excerpt', 'sectionId', 'title', 'url']);
  assert.match(result.url, /^docs\.html#[a-z-]+$/);
  assert.doesNotMatch(result.excerpt, /<(?:a|code|strong|em|br|span|p)\b/i);
}
assert.equal(webMcpTools.search_documentation.execute({ query: '   ' }).results.length, 0);
assert.ok(webMcpTools.search_documentation.execute({ query: 'x'.repeat(201) }).error);
assert.ok(webMcpTools.search_documentation.execute({ query: `${' '.repeat(200)}x` }).error);
assert.equal(webMcpTools.search_documentation.execute({ query: 'p table' }).results.length, 0);
const commandMatches = webMcpTools.search_documentation.execute({ query: 'nanh <harness>' });
assert.ok(commandMatches.results.some(({ excerpt }) => excerpt.includes('nanh <harness>')));

const opened = webMcpTools.open_documentation.execute({ sectionId: 'harnesses' });
assert.equal(opened.sectionId, 'harnesses');
assert.equal(opened.url, 'docs.html#harnesses');
assert.equal(openedDocumentationUrl, 'docs.html#harnesses');
openedDocumentationUrl = undefined;
const invalidOpen = webMcpTools.open_documentation.execute({ sectionId: 'unknown' });
assert.ok(invalidOpen.error);
assert.equal(openedDocumentationUrl, undefined);

const pagehide = webMcpEvents.find(({ type }) => type === 'pagehide');
assert.ok(pagehide);
assert.ok(webMcpRegistrations.every(({ options }) => options?.signal));
pagehide.listener({ persisted: true });
assert.equal(webMcpRegistrations[0].options.signal.aborted, false);
pagehide.listener({ persisted: false });
assert.equal(webMcpRegistrations[0].options.signal.aborted, true);

for (const location of [undefined, { assign() { throw new Error('synthetic navigation failure'); } }]) {
  const registrations = [];
  renderWeb('landing', 'en', orderedScripts, {
    modelContext: { registerTool(tool) { registrations.push(tool); } },
    window: { location },
  });
  const navigation = registrations.find(({ name }) => name === 'open_documentation');
  assert.ok(navigation.execute({ sectionId: 'install' }).error);
}

const spanishTools = [];
const spanishRender = renderWeb('docs', 'es', orderedScripts, {
  modelContext: { registerTool(tool) { spanishTools.push(tool); } },
});
const spanishTitle = spanishRender.copy.docsSections[0][1];
const spanishSearch = spanishTools.find(({ name }) => name === 'search_documentation');
assert.equal(spanishSearch.execute({ query: spanishTitle }).results[0].title, spanishTitle);
assert.equal(renderWeb('landing', 'en', orderedScripts, {
  modelContext: { registerTool() { throw new Error('synthetic registration failure'); } },
}).html, landing);

const rejectedRegistrationPromises = [];
const unhandledRejections = [];
const onUnhandledRejection = (reason) => unhandledRejections.push(reason);
process.on('unhandledRejection', onUnhandledRejection);
try {
  renderWeb('landing', 'en', orderedScripts, {
    AbortController,
    modelContext: {
      registerTool() {
        const rejection = Promise.reject(new Error('synthetic registration rejection'));
        rejectedRegistrationPromises.push(rejection);
        return rejection;
      },
    },
  });
  await Promise.allSettled(rejectedRegistrationPromises);
  await new Promise((resolve) => setImmediate(resolve));
} finally {
  process.off('unhandledRejection', onUnhandledRejection);
}
assert.equal(rejectedRegistrationPromises.length, 2);
assert.equal(unhandledRejections.length, 0);

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
