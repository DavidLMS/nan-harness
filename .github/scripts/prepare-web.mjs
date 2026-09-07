import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { renderWeb } from './render-web.mjs';
import { pathToFileURL } from 'node:url';

const siteOrigin = 'https://nan-harness.davidlms.com';
const skillSchemaUrl = 'https://schemas.agentskills.io/discovery/0.2.0/schema.json';

function inlineMarkdown(value) {
  return value
    .replace(/<a\b[^>]*href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/g,
      (_match, href, text) => `[${text}](${new URL(href, `${siteOrigin}/`).href})`)
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
  const cells = (row) => `| ${row.map((cell) => inlineMarkdown(cell).replace(/\|/g, '\\|')).join(' | ')} |`;
  return [
    cells(headers),
    `| ${headers.map(() => '---').join(' | ')} |`,
    ...rows.map(cells),
  ].join('\n');
}

function markdownCode(commands, language = '') {
  return [`\`\`\`${language}`, ...commands, '```'].join('\n');
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

## ${copy.whatIs}

${inlineMarkdown(copy.heroLede)}

${copy.whatText}

## ${copy.installLatest}

${markdownCode([copy.installCommand], 'sh')}

${markdownCode([copy.installWindowsCommand], 'powershell')}

## ${copy.workflowLabel}

${inlineMarkdown(copy.workflowText)}

${markdownCode(['nanh <harness>', 'nanh claude', 'nanh codex --model qwen3.6'])}

## ${copy.telemetryLabel}

${inlineMarkdown(copy.telemetryText)}

${markdownCode([copy.telemetryCommand])}

## FAQ

${copy.faqs.map(([question, answer]) => `### ${inlineMarkdown(question)}\n\n${inlineMarkdown(answer)}`).join('\n\n')}

## ${copy.readDocs}

[${copy.docsNavigation}](${siteOrigin}${locale === 'en' ? '' : '/es'}/docs.md)
`;
}

function docsMarkdown(copy, locale) {
  const sections = copy.docsSections.map(([, title, blocks]) => {
    return [`## ${inlineMarkdown(title)}`, blocks.map(markdownBlock).join('\n\n')].join('\n\n');
  });
  return `# ${copy.docsTitle}

${copy.docsMeta}

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
  const { copy } = renderWeb(page, locale);
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

function writeSkills(destination) {
  // Draft discovery 0.2.0: https://github.com/cloudflare/agent-skills-discovery-rfc
  const skills = [
    {
      name: 'nan-harness-overview',
      description: 'Explain nan-harness and guide installation or first use when someone wants to run an existing coding harness with NaN.',
      instructions: `# Start with nan-harness

Read the [overview](${siteOrigin}/index.md) for installation and the
[CLI documentation](${siteOrigin}/docs.md) for the requested platform or harness.
These generated documents share the website's maintained content.

nan-harness connects existing coding harnesses to NaN; it does not replace their
interfaces. Prefer managed launches with \`nanh <harness>\`. Native setup through
\`nanh config <harness>\` is an advanced, persistent configuration operation.

For an explanation, provide instructions without installing software or changing
configuration. If installation or a launch is requested, use the matching platform
instructions and preserve the requested harness. Do not request API keys in chat;
direct credential entry to the local authentication flow. Do not enable telemetry
or private diagnostic capture unless requested.`,
    },
    {
      name: 'nan-harness-cli-docs',
      description: 'Help with nan-harness commands, native setup, compatibility, or troubleshooting using its maintained CLI documentation.',
      instructions: `# Use nan-harness documentation

Read the relevant section of the [CLI documentation](${siteOrigin}/docs.md).
The [Spanish documentation](${siteOrigin}/es/docs.md) covers the same commands.
Use the installed \`nanh --help\` and subcommand help when behavior depends on
the installed version; report discrepancies instead of guessing flags.

Distinguish managed launches (\`nanh <harness>\`) from persistent native setup
(\`nanh config <harness>\`). Preserve user-owned configuration and do not switch
workflows without a reason grounded in the request. Models available to the
account come from live discovery, not a fixed list in these documents.

Diagnose with the documented compatibility and status commands. An explanation
does not authorize installation, configuration changes, paid model calls, or
uploading diagnostics. Never include credentials, prompts, model output, or
private capture files in reports.`,
    },
  ];
  const index = {
    $schema: skillSchemaUrl,
    name: 'nan-harness',
    description: 'Agent-readable documentation for installing and running AI coding harnesses with NaN.',
    skills: skills.map(({ name, description, instructions }) => {
      const file = `.well-known/agent-skills/${name}/SKILL.md`;
      const contents = `---\nname: ${name}\ndescription: ${description}\n---\n\n${instructions}\n`;
      fs.mkdirSync(path.dirname(path.join(destination, file)), { recursive: true });
      fs.writeFileSync(path.join(destination, file), contents);
      return {
        name, description, type: 'skill-md', url: `${siteOrigin}/${file}`,
        digest: `sha256:${crypto.createHash('sha256').update(contents).digest('hex')}`,
      };
    }),
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
  writeSkills(destination);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const destination = process.argv[2];
  if (!destination) throw new Error('Usage: node prepare-web.mjs <staging-directory>');
  prepareWeb(destination);
}
