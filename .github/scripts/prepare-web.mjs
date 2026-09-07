import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import vm from 'node:vm';
import { pathToFileURL } from 'node:url';

const siteOrigin = 'https://davidlms.github.io/nan-harness';
const skillSchemaUrl = 'https://agentskills.io/schemas/agent-skills-index/v0.2.0.json';
const releaseDownloadBase = 'https://github.com/DavidLMS/nan-harness/releases/latest/download';
const unixInstallCommand = `curl --proto '=https' --tlsv1.2 -m 30 -fsSL ${releaseDownloadBase}/install.sh | sh`;
const windowsInstallCommand = `irm ${releaseDownloadBase}/install.ps1 | iex`;
const harnessSites = {
  claude: 'https://www.anthropic.com/claude-code',
  codex: 'https://openai.com/codex/',
  opencode: 'https://opencode.ai/',
  hermes: 'https://hermes-agent.nousresearch.com/',
  omp: 'https://omp.sh/',
  pi: 'https://pi.dev/',
  prime: 'https://github.com/PrimeIntellect-ai/prime-agent',
  deepseek: 'https://deepseek.com/harness/en/',
  openclaw: 'https://openclaw.ai/',
  cline: 'https://cline.bot/',
  qwen: 'https://qwenlm.github.io/qwen-code-docs/en/users/overview',
  kimi: 'https://www.kimi.com/code',
  aider: 'https://aider.chat/',
  goose: 'https://github.com/block/goose',
  fx: 'https://fx.sh/',
};

function loadContent(locale) {
  const source = fs.readFileSync(path.join('web', `content-${locale}.js`), 'utf8');
  const factoryName = locale === 'en' ? 'nanHarnessContentEn' : 'nanHarnessContentEs';
  const contextSource = `${source}\nconst harnessSites = ${JSON.stringify(harnessSites)};\n${factoryName}({
      harnessLink: (label, harness) => '<a href="' + harnessSites[harness] + '">' + label + '</a>',
      nanLink: (label) => '<a href="https://nan.builders/">' + label + '</a>',
      unixInstallCommand: ${JSON.stringify(unixInstallCommand)},
      windowsInstallCommand: ${JSON.stringify(windowsInstallCommand)},
    })`;
  return vm.runInNewContext(contextSource, {}, {
    filename: `content-${locale}.js`,
  });
}

function inlineMarkdown(value) {
  return value
    .replace(/<a\b[^>]*href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/g, (_match, href, text) => `[${text}](${href})`)
    .replace(/<code>([\s\S]*?)<\/code>/g, '`$1`')
    .replace(/<\/?(?:strong|em|br|span|p)\b[^>]*>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
    .trim();
}

function markdownTable(headers, rows) {
  const cells = (row) => `| ${row.map(inlineMarkdown).join(' | ')} |`;
  return [
    cells(headers),
    `| ${headers.map(() => '---').join(' | ')} |`,
    ...rows.map(cells),
  ].join('\n');
}

function markdownCode(commands) {
  return ['```sh', ...commands, '```'].join('\n');
}

function markdownBlock([kind, value, rows]) {
  if (kind === 'p') return inlineMarkdown(value);
  if (kind === 'h3') return `### ${inlineMarkdown(value)}`;
  if (kind === 'note') return `> ${inlineMarkdown(value)}`;
  if (kind === 'code') return markdownCode([value]);
  if (kind === 'codes') return markdownCode(value);
  if (kind === 'table') return markdownTable(value, rows);
  return inlineMarkdown(value);
}

function landingMarkdown(copy, locale) {
  return `# ${copy.siteTitle}

${copy.siteMeta}

- HTML: ${siteOrigin}/
- Markdown: ${siteOrigin}${locale === 'en' ? '/index.md' : '/es/index.md'}
- Documentation: ${siteOrigin}/docs.html

## What it does

${inlineMarkdown(copy.heroLede)}

${copy.whatText}

## Install the latest release

${markdownCode([copy.installCommand, copy.installWindowsCommand])}

## Recommended workflow

${inlineMarkdown(copy.workflowText)}

${markdownCode(['nanh <harness>', 'nanh claude', 'nanh codex --model qwen3.6'])}

## Privacy

${inlineMarkdown(copy.telemetryText)}

${markdownCode([copy.telemetryCommand])}

## FAQ

${copy.faqs.map(([question, answer]) => `### ${inlineMarkdown(question)}\n\n${inlineMarkdown(answer)}`).join('\n\n')}

## Start with the docs

Read the generated documentation at ${siteOrigin}/docs.md.
`;
}

function docsMarkdown(copy, locale) {
  const sections = copy.docsSections.map(([id, title, blocks]) => {
    return [`### ${inlineMarkdown(title)}`, blocks.map(markdownBlock).join('\n\n')].join('\n\n');
  });
  return `# ${copy.docsTitle}

${copy.docsLede}

- HTML: ${siteOrigin}/docs.html
- Markdown: ${siteOrigin}${locale === 'en' ? '/docs.md' : '/es/docs.md'}

${inlineMarkdown(copy.docsIntro)}

${sections.join('\n\n')}
`;
}

function logosMarkdown(copy) {
  return `# ${copy.logosHeading}

${inlineMarkdown(copy.logosIntro)}

## ${copy.logosSimpleIconsHeading}

${inlineMarkdown(copy.logosSimpleIconsText)}

${inlineMarkdown(copy.logosPackage)}

## ${copy.logosOfficialHeading}

${inlineMarkdown(copy.logosOfficialText)}

${markdownTable(copy.logosTableHeaders, copy.logosSources)}

${inlineMarkdown(copy.logosLicenseText)}
`;
}

function markdown(page, locale) {
  const copy = loadContent(locale);
  return page === 'landing'
    ? landingMarkdown(copy, locale)
    : page === 'docs' ? docsMarkdown(copy, locale) : logosMarkdown(copy);
}

function lastModifiedDate() {
  return execFileSync('git', ['log', '-1', '--format=%cI'], { encoding: 'utf8' }).trim().slice(0, 10);
}

function writeSitemap(destination) {
  const modified = lastModifiedDate();
  const urls = ['/', '/docs.html', '/logos.html'].map((sitePath) => {
    return `  <url>\n    <loc>${siteOrigin}${sitePath}</loc>\n    <lastmod>${modified}</lastmod>\n  </url>`;
  });
  fs.writeFileSync(
    path.join(destination, 'sitemap.xml'),
    [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">',
      ...urls,
      '</urlset>',
      '',
    ].join('\n'),
  );
}

function writeSkills(destination, files) {
  const skills = [
    {
      name: 'nan-harness-overview',
      type: 'text/markdown',
      description: 'Explains what nan-harness is, how to install it, and how to run a supported coding agent with NaN.',
      file: 'index.md',
    },
    {
      name: 'nan-harness-cli-docs',
      type: 'text/markdown',
      description: 'Documents the recommended workflow, native setup, desktop integrations, search policy, and privacy options.',
      file: 'docs.md',
    },
  ];
  const index = {
    $schema: skillSchemaUrl,
    name: 'nan-harness',
    description: 'Agent-readable documentation for installing and running AI coding harnesses with NaN.',
    skills: skills.map(({ file, ...skill }) => ({
      ...skill,
      url: `${siteOrigin}/${file}`,
      sha256: crypto.createHash('sha256').update(files.get(file)).digest('hex'),
    })),
  };
  fs.mkdirSync(path.join(destination, '.well-known', 'agent-skills'), { recursive: true });
  fs.writeFileSync(
    path.join(destination, '.well-known', 'agent-skills', 'index.json'),
    `${JSON.stringify(index, null, 2)}\n`,
  );
}

export function prepareWeb(destination) {
  fs.cpSync('web', destination, { recursive: true });
  // Cached HTML may load app.js without the newly extracted locale scripts.
  const source = ['content-en.js', 'content-es.js', 'app.js']
    .map((name) => fs.readFileSync(path.join('web', name), 'utf8'))
    .join('\n;\n');
  fs.writeFileSync(path.join(destination, 'app.js'), source);

  const files = new Map([
    ['index.md', markdown('landing', 'en')],
    ['docs.md', markdown('docs', 'en')],
    ['logos.md', markdown('logos', 'en')],
    ['es/index.md', markdown('landing', 'es')],
    ['es/docs.md', markdown('docs', 'es')],
    ['es/logos.md', markdown('logos', 'es')],
  ]);
  fs.mkdirSync(path.join(destination, 'es'), { recursive: true });
  for (const [relativePath, contents] of files) {
    fs.writeFileSync(path.join(destination, relativePath), contents);
  }
  writeSitemap(destination);
  writeSkills(destination, files);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const destination = process.argv[2];
  if (!destination) throw new Error('Usage: node prepare-web.mjs <staging-directory>');
  prepareWeb(destination);
}
