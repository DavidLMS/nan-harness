const AUTOPLAY_INITIAL_DELAY_MS = 1000;
const AUTOPLAY_INTERVAL_MS = 3000;

const harnesses = [
  ['claude', 'Claude Code', 'nanh claude', 'C', 'logos/claude.svg'],
  ['codex', 'Codex', 'nanh codex', '>_', 'logos/codex.png'],
  ['opencode', 'OpenCode', 'nanh opencode', '□', 'logos/opencode.svg'],
  ['hermes', 'Hermes', 'nanh hermes', 'H', 'logos/hermes.png'],
  ['omp', 'Oh My Pi', 'nanh omp', 'π', 'logos/omp.svg'],
  ['pi', 'Pi', 'nanh pi', 'π', 'logos/pi.svg'],
  ['prime', 'Prime Agent', 'nanh prime-agent', 'P', 'logos/prime.svg'],
  ['deepseek', 'DeepSeek', 'nanh dsh', 'D', 'logos/deepseek.svg'],
  ['openclaw', 'OpenClaw', 'nanh openclaw', '◈', 'logos/openclaw.svg'],
  ['cline', 'Cline', 'nanh cline', 'CL', 'logos/cline.svg'],
  ['qwen', 'Qwen Code', 'nanh qwen', 'Q', 'logos/qwen.svg'],
  ['kimi', 'Kimi Code', 'nanh kimi', 'K', 'logos/kimi.svg'],
  ['aider', 'Aider', 'nanh aider', 'A', 'logos/aider.svg'],
  ['goose', 'Goose', 'nanh goose', 'G', 'logos/goose.svg'],
  ['fx', 'fx', 'nanh fx', 'fx', 'logos/fx.svg']
];

const releaseDownloadBase = 'https://github.com/DavidLMS/nan-harness/releases/latest/download';
const unixInstallCommand = `curl --proto '=https' --tlsv1.2 -m 30 -fsSL ${releaseDownloadBase}/install.sh | sh`;
const windowsInstallCommand = `irm ${releaseDownloadBase}/install.ps1 | iex`;
const githubUrl = 'https://github.com/DavidLMS/nan-harness';
const nanProviderUrl = 'https://nan.builders/';
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
  fx: 'https://fx.sh/'
};

function harnessLink(label, harness) {
  return `<a href="${harnessSites[harness]}" target="_blank" rel="noreferrer">${label}</a>`;
}

function nanLink(label) {
  return `<a class="nan-provider-link" href="${nanProviderUrl}" target="_blank" rel="noreferrer">${label}</a>`;
}

const localeContent = { harnessLink, nanLink, unixInstallCommand, windowsInstallCommand };

const translations = {
  en: nanHarnessContentEn(localeContent),
  es: nanHarnessContentEs(localeContent)
};

function detectLocale() {
  try {
    const saved = window.localStorage.getItem('nan-harness-locale');
    if (saved === 'en' || saved === 'es') return saved;
  } catch {}
  return (navigator.languages || [navigator.language]).some((language) => language.toLowerCase().startsWith('es')) ? 'es' : 'en';
}

let currentLocale = detectLocale();

function t(key) {
  return translations[currentLocale][key] ?? translations.en[key] ?? key;
}

const WEB_MCP_MAX_QUERY_LENGTH = 200;
const WEB_MCP_MAX_EXCERPT_LENGTH = 240;

function plainText(value) {
  if (Array.isArray(value)) return value.map(plainText).join(' ');
  if (value == null) return '';
  return String(value)
    .replace(/<[^>]*>/g, ' ')
    .replace(/&(?:amp|lt|gt|quot|#39|#x27);/gi, (entity) => ({
      '&amp;': '&',
      '&lt;': '<',
      '&gt;': '>',
      '&quot;': '"',
      '&#39;': "'",
      '&#x27;': "'",
    }[entity.toLowerCase()] ?? ' '))
    .replace(/\s+/g, ' ')
    .trim();
}

function documentationRecords() {
  return (translations[currentLocale].docsSections ?? []).map(([sectionId, title, blocks]) => {
    const plainTitle = plainText(title);
    const content = plainText(blocks.map(([, value, rows]) => [value, rows]));
    return {
      sectionId,
      title: plainTitle,
      content,
      searchableText: `${plainTitle}. ${content}`.trim(),
    };
  });
}

function documentationExcerpt(text, query) {
  // Records are already plain text; stripping again would erase CLI placeholders.
  const normalizedText = text;
  if (normalizedText.length <= WEB_MCP_MAX_EXCERPT_LENGTH) return normalizedText;
  const matchIndex = normalizedText.toLowerCase().indexOf(query.toLowerCase());
  const start = matchIndex < 0
    ? 0
    : Math.min(Math.max(0, matchIndex - 72), normalizedText.length - WEB_MCP_MAX_EXCERPT_LENGTH);
  const end = start + WEB_MCP_MAX_EXCERPT_LENGTH;
  return `${start > 0 ? '…' : ''}${normalizedText.slice(start, end).trim()}${end < normalizedText.length ? '…' : ''}`;
}

function searchDocumentation(input) {
  const rawQuery = typeof input?.query === 'string' ? input.query : '';
  if (!rawQuery.trim() || rawQuery.length > WEB_MCP_MAX_QUERY_LENGTH) {
    return { error: 'Query must be a non-empty string of at most 200 characters.', results: [] };
  }

  const query = rawQuery.trim();
  const needle = query.toLowerCase();
  const results = documentationRecords()
    .filter(({ searchableText }) => searchableText.toLowerCase().includes(needle))
    .slice(0, 5)
    .map(({ sectionId, title, searchableText }) => ({
      sectionId,
      title,
      excerpt: documentationExcerpt(searchableText, query),
      url: `docs.html#${sectionId}`,
    }));
  return { query, results };
}

function openDocumentation(input) {
  const sectionId = typeof input?.sectionId === 'string' ? input.sectionId : '';
  const section = documentationRecords().find((record) => record.sectionId === sectionId);
  if (!section) return { error: 'Unknown documentation section.', sectionId };

  const url = `docs.html#${section.sectionId}`;
  if (typeof window.location?.assign !== 'function') {
    return { error: 'Navigation is unavailable. Open the documentation URL directly.', url };
  }
  try {
    window.location.assign(url);
  } catch {
    return { error: 'Navigation failed. Open the documentation URL directly.', url };
  }
  return { sectionId: section.sectionId, title: section.title, url };
}

function registerWebMcpTools() {
  const modelContext = document.modelContext;
  if (typeof modelContext?.registerTool !== 'function') return;

  const registrationController = typeof AbortController === 'function' ? new AbortController() : null;
  const registrationOptions = registrationController ? { signal: registrationController.signal } : undefined;
  const tools = [
    {
      name: 'search_documentation',
      description: 'Search the current language of nan-harness documentation by title and content.',
      inputSchema: {
        type: 'object',
        properties: {
          query: {
            type: 'string',
            minLength: 1,
            maxLength: WEB_MCP_MAX_QUERY_LENGTH,
            description: 'A non-empty documentation search query.',
          },
        },
        required: ['query'],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: true },
      execute: searchDocumentation,
    },
    {
      name: 'open_documentation',
      description: 'Navigate the current browser tab to a nan-harness documentation section in the selected language.',
      inputSchema: {
        type: 'object',
        properties: {
          sectionId: {
            type: 'string',
            enum: documentationRecords().map(({ sectionId }) => sectionId),
            description: 'The documentation section identifier to open.',
          },
        },
        required: ['sectionId'],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: false },
      execute: openDocumentation,
    },
  ];

  for (const tool of tools) {
    try {
      const registration = registrationOptions
        ? modelContext.registerTool(tool, registrationOptions)
        : modelContext.registerTool(tool);
      Promise.resolve(registration).catch(() => {});
    } catch {}
  }

  // BFCache restores this same document without running registration again.
  window.addEventListener?.('pagehide', (event) => {
    if (!event.persisted) registrationController?.abort();
  });
}

function wordmark() {
  return '<span class="wordmark"><b>nan</b><i>-</i><strong>harness</strong></span>';
}

function arrow() {
  return '<span aria-hidden="true">→</span>';
}

function githubLink() {
  return `<a class="github-link" href="${githubUrl}" target="_blank" rel="noreferrer" aria-label="${t('githubAria')}"><svg class="github-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M8 1.1a6.9 6.9 0 0 0-2.2 13.44c.35.07.48-.15.48-.34v-1.2c-1.97.43-2.39-.95-2.39-.95-.32-.82-.79-1.04-.79-1.04-.64-.44.05-.43.05-.43.71.05 1.08.73 1.08.73.63 1.08 1.65.77 2.05.59.06-.46.25-.77.45-.95-1.57-.18-3.22-.79-3.22-3.5 0-.77.28-1.4.73-1.89-.07-.18-.32-.9.07-1.87 0 0 .6-.19 1.98.72a6.8 6.8 0 0 1 3.6 0c1.38-.91 1.98-.72 1.98-.72.39.97.14 1.69.07 1.87.45.49.73 1.12.73 1.89 0 2.72-1.66 3.32-3.24 3.5.25.22.48.64.48 1.29v1.92c0 .19.13.41.49.34A6.9 6.9 0 0 0 8 1.1Z"></path></svg><span>GITHUB</span><span class="github-arrow" aria-hidden="true">↗</span></a>`;
}

function languageSelector() {
  return `<div class="language-selector" role="group" aria-label="${t('language')}"><button type="button" data-locale="en" aria-pressed="${currentLocale === 'en'}">EN</button><span>/</span><button type="button" data-locale="es" aria-pressed="${currentLocale === 'es'}">ES</button></div>`;
}

function nav(page = 'landing') {
  return `<a class="skip-link" href="#main-content">${t('skipToContent')}</a><header class="site-header">
    <a class="brand-link" href="index.html" aria-label="${t('homeAria')}">${wordmark()}</a>
    <nav class="main-nav" aria-label="${t('mainNavigation')}">
      <a href="docs.html">${t('docs')}</a><a href="index.html#faq">${t('faq')}</a>${githubLink()}${languageSelector()}
    </nav>
    <a class="header-cta" href="docs.html">${t('getStarted')} ${arrow()}</a>
  </header>`;
}

function heroArt() {
  const pickerItems = Array.from({ length: 5 }, (_, cycle) => harnesses.map(([name, meta, , mark, logo], index) => {
    const position = cycle * harnesses.length + index;
    const selected = cycle === 2 && index === 0;
    return `<div class="picker-item ${selected ? 'is-active' : ''}" data-picker-item data-index="${position}" data-logical-index="${index}">
      <span class="picker-logo picker-logo-${name}" data-fallback="${mark}">${logo ? `<img src="${logo}" alt="" loading="eager" />` : mark}</span><span class="picker-item-copy"><strong>${meta}</strong></span>
    </div>`;
  }).join('')).join('');
  const pickerOptions = harnesses.map(([name, meta], index) => `<span class="sr-only" id="picker-option-${name}" data-picker-option data-logical-index="${index}" role="option" aria-selected="${index === 0}">${meta}</span>`).join('');
  return `<div class="hero-art picker-art" data-picker>
    <button class="picker-autoplay" type="button" data-picker-autoplay data-state="playing" aria-label="${t('pauseCarousel')}" title="${t('pauseCarousel')}"><span class="picker-autoplay-icons" aria-hidden="true"><svg class="picker-autoplay-pause" viewBox="0 0 16 16"><rect x="4" y="3" width="2" height="10" rx="1"></rect><rect x="10" y="3" width="2" height="10" rx="1"></rect></svg><svg class="picker-autoplay-play" viewBox="0 0 16 16"><path d="M5 3.6 12 8l-7 4.4Z"></path></svg></span></button>
    <div class="picker-frame" data-picker-control role="listbox" tabindex="0" aria-label="${t('chooseHarness')}" aria-activedescendant="picker-option-claude">
      <div class="picker-glow"></div><div class="picker-fade picker-fade-top"></div><div class="picker-fade picker-fade-bottom"></div>
      <div class="picker-track" data-picker-track aria-hidden="true">
        <div class="picker-spacer" aria-hidden="true"></div>
        ${pickerItems}
        <div class="picker-spacer" aria-hidden="true"></div>
      </div>
      ${pickerOptions}
      <div class="picker-center-line" aria-hidden="true"></div>
    </div>
  </div>`;
}

function routeCommand() {
  return `<div class="route-command" data-picker-command hidden><span class="route-command-prompt" aria-hidden="true">$</span><code data-picker-command-text></code><button type="button" data-picker-copy data-state="copy" aria-label="${t('copyCommand')}" title="${t('copyCommand')}"><svg class="copy-icon" viewBox="0 0 16 16" aria-hidden="true"><rect x="5" y="2.5" width="8" height="9" rx="1.2"></rect><path d="M3 5.5v7A1.5 1.5 0 0 0 4.5 14H10"></path></svg><svg class="check-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="m3 8.2 3.1 3.1L13 4.8"></path></svg></button><small class="sr-only" data-picker-copy-status role="status" aria-live="polite"></small></div>`;
}

function heroNanDots() {
  const letters = [
    ['10001', '11001', '10101', '10011', '10001', '10001', '10001'],
    ['01110', '10001', '10001', '11111', '10001', '10001', '10001'],
    ['10001', '11001', '10101', '10011', '10001', '10001', '10001']
  ];
  const dots = [];
  const dotSize = 15;
  const startX = 330;
  const startY = 232;

  letters.forEach((letter, letterIndex) => {
    letter.forEach((row, rowIndex) => {
      [...row].forEach((filled, columnIndex) => {
        if (filled !== '1') return;
        const variation = (letterIndex * 5 + rowIndex * 3 + columnIndex) % 4;
        const centerX = startX + (letterIndex * 6 + columnIndex) * dotSize;
        const centerY = startY + rowIndex * dotSize;
        const radius = 2.15 + variation * .14;
        const opacity = .58 + variation * .1;
        dots.push(`<circle class="hero-nan-dot" cx="${centerX - 3.2}" cy="${centerY}" r="${radius}" opacity="${opacity}"/><circle class="hero-nan-dot" cx="${centerX + 3.2}" cy="${centerY}" r="${radius}" opacity="${opacity}"/>`);
      });
    });
  });

  return dots.join('');
}

function heroMeltTrail(centerX, startY, index) {
  const targetX = 336 + index * 32;
  const targetY = 220 + (index % 3) * 9;
  const controlX = centerX + (targetX - centerX) * .16;
  const controlY = startY + 128;
  return Array.from({ length: 24 }, (_, step) => {
    const progress = step / 23;
    const inverse = 1 - progress;
    const sway = Math.sin(step * 1.21 + index) * (2.4 + progress * 5.5);
    const x = inverse * inverse * centerX + 2 * inverse * progress * controlX + progress * progress * targetX + sway;
    const y = inverse * inverse * (startY + 26) + 2 * inverse * progress * controlY + progress * progress * targetY;
    const radius = 2.5 - progress * 1.5 + (step % 4 === 0 ? .35 : 0);
    const opacity = .74 - progress * .44;
    const delay = step * 72;
    return `<circle class="hero-melt-dot" style="--hero-melt-delay:${delay}ms" cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="${radius.toFixed(1)}" opacity="${opacity.toFixed(2)}"/>`;
  }).join('');
}

function heroDustField() {
  let seed = 20260820;
  const rand = () => { seed = (seed * 1664525 + 1013904223) % 4294967296; return seed / 4294967296; };
  const dots = [];
  for (let i = 0; i < 240; i += 1) {
    const x = 130 + rand() * 500;
    const y = 18 + rand() * 358;
    const edge = x > 430 ? 1 : .6;
    const radius = .8 + rand() * 1.5;
    const opacity = (.14 + rand() * .32) * edge;
    const delay = -((i * 173) % 8000);
    dots.push(`<circle class="hero-dust-dot" style="--hero-dust-delay:${delay}ms" cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="${radius.toFixed(1)}" opacity="${opacity.toFixed(2)}"/>`);
  }
  return dots.join('');
}

function heroVisual() {
  const logoSlots = [
    ['claude', 'logos/claude.svg', 176, 26],
    ['codex', 'logos/codex.png', 228, 38],
    ['opencode', 'logos/opencode.svg', 280, 31],
    ['hermes', 'logos/hermes.png', 332, 46],
    ['pi', 'logos/pi.svg', 384, 39],
    ['prime', 'logos/prime.svg', 436, 55],
    ['kimi', 'logos/kimi.svg', 488, 48],
    ['goose', 'logos/goose.svg', 540, 64]
  ];
  const logos = logoSlots.map(([name, source, x, y], index) => `<g class="hero-melt-logo hero-melt-logo-${name}" style="--hero-logo-delay:${-(index * 460)}ms" opacity="${(1 - index * .055).toFixed(2)}"><image href="${source}" x="${x}" y="${y}" width="25" height="25" preserveAspectRatio="xMidYMid meet"/><g>${heroMeltTrail(x + 12.5, y, index)}</g></g>`).join('');

  return `<div class="hero-visual" aria-hidden="true">
    <svg viewBox="0 0 620 390" role="presentation">
      <defs>
        <radialGradient id="hero-dot-haze"><stop stop-color="#faca88" stop-opacity=".26"/><stop offset="1" stop-color="#ca8631" stop-opacity="0"/></radialGradient>
        <filter id="hero-logo-glow" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="1.2" result="blur"/><feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
      </defs>
      <ellipse class="hero-dot-haze" cx="250" cy="262" rx="168" ry="108"/>
      <ellipse class="hero-dot-haze hero-dot-haze-right" cx="486" cy="264" rx="186" ry="150"/>
      <g class="hero-dust">${heroDustField()}</g>
      <g class="hero-melt-logos">${logos}</g>
      <g class="hero-visual-ascii">${heroNanDots()}</g>
      <g class="hero-melt-scatter">
        <circle cx="144" cy="222" r="1.7"/><circle cx="476" cy="206" r="1.4"/><circle cx="117" cy="251" r="1.1"/><circle cx="493" cy="239" r="1.8"/><circle cx="151" cy="291" r="1.2"/><circle cx="465" cy="294" r="1.1"/><circle cx="591" cy="132" r="1.5"/><circle cx="603" cy="210" r="1.1"/><circle cx="592" cy="326" r="1.6"/><circle cx="616" cy="104" r="1.2"/><circle cx="619" cy="180" r="1.5"/><circle cx="614" cy="276" r="1.1"/><circle cx="618" cy="348" r="1.4"/>
      </g>
    </svg>
  </div>`;
}

function detectInstallTarget() {
  return /Windows/i.test(navigator.userAgent) ? 'windows' : 'unix';
}

function installTargetCommand(target) {
  return target === 'windows' ? t('installWindowsCommand') : t('installCommand');
}

function installCommand() {
  const target = detectInstallTarget();
  const command = installTargetCommand(target);
  const prompt = target === 'windows' ? 'PS>' : '$';
  return `<div class="install-command code-block" data-install-command data-install-target="${target}"><div class="install-command-shell"><div class="install-tabs" role="tablist" aria-label="${t('installPlatform')}"><button id="install-tab-unix" type="button" role="tab" data-install-tab="unix" aria-controls="install-command-panel" aria-selected="${target === 'unix'}" tabindex="${target === 'unix' ? '0' : '-1'}">${t('unixTab')}</button><button id="install-tab-windows" type="button" role="tab" data-install-tab="windows" aria-controls="install-command-panel" aria-selected="${target === 'windows'}" tabindex="${target === 'windows' ? '0' : '-1'}">${t('windowsTab')}</button></div><div class="install-command-head"><span>${t('installLatest')}</span><button type="button" data-copy="${command}" data-state="copy" aria-label="${t('copyCommand')}" title="${t('copyCommand')}"><svg class="copy-icon" viewBox="0 0 16 16" aria-hidden="true"><rect x="5" y="2.5" width="8" height="9" rx="1.2"></rect><path d="M3 5.5v7A1.5 1.5 0 0 0 4.5 14H10"></path></svg><svg class="check-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="m3 8.2 3.1 3.1L13 4.8"></path></svg></button></div></div><code id="install-command-panel" role="tabpanel" aria-labelledby="install-tab-${target}" data-install-code><b>${prompt}</b> ${command}</code><small class="copy-status" role="status" aria-live="polite"></small></div>`;
}

async function writeClipboard(value) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return;
    }
  } catch {}

  const input = document.createElement('textarea');
  input.value = value;
  input.setAttribute('readonly', '');
  input.style.position = 'fixed';
  input.style.opacity = '0';
  document.body.appendChild(input);
  input.select();
  try {
    if (!document.execCommand || !document.execCommand('copy')) throw new Error('copy failed');
  } finally {
    input.remove();
  }
}

function faqRows() {
  return t('faqs').map(([question, answer]) => `<details class="faq-row"><summary>${question}<span>+</span></summary><p>${answer}</p></details>`).join('');
}

function footer() {
  return `<footer class="site-footer"><div>${wordmark()}<p>${t('footerTagline')}</p></div><a href="logos.html">${t('logoNotices')}</a></footer>`;
}

function telemetryArt() {
  return `<svg class="telemetry-icon" viewBox="0 0 120 120" role="presentation" aria-hidden="true">
    <path class="ic ic-dim" d="M23.6 57 A42 42 0 0 1 96.4 57"/>
    <path class="ic" d="M34 63 A30 30 0 0 1 86 63" opacity=".62"/>
    <path class="ic" d="M44.4 69 A18 18 0 0 1 75.6 69"/>
    <circle class="ic-fill" cx="60" cy="78" r="5"/>
    <path class="ic-cut" d="M26 94 L94 26"/>
    <path class="ic" d="M26 94 L94 26"/>
  </svg>`;
}

function landing() {
  const heroTitle = t('heroTitle').split('|');
  const faqHeading = t('faqHeading').split('|');
  return `${nav()}<main id="main-content">
    <section class="hero page-width"><div class="hero-copy"><h1>${heroTitle[0]}<br><em>${heroTitle[1]}</em><br>${heroTitle[2].replace('NAN.', '<em>NAN.</em>')}</h1><p class="hero-lede">${t('heroLede')}</p><div class="hero-actions"><a class="purple-button" href="docs.html">${t('readDocs')} ${arrow()}</a><a class="text-link" href="#harness-picker">${t('seeHarnesses')} ${arrow()}</a></div>${installCommand()}</div>${heroVisual()}<div class="hero-route-row" id="harness-picker"><div class="hero-nan-lockup"><span>nanh</span></div>${heroArt()}${routeCommand()}</div></section>

    <section class="section-space community-section page-width"><div class="section-number">01 <span>${t('whatIs')}</span></div><div class="section-copy"><h2>${t('oneLocal')}<br><em>${t('everyAgent')}</em></h2><p>${t('whatText')}</p><a class="text-link" href="docs.html#harnesses">${t('howItWorks')} ${arrow()}</a></div></section>

    <section class="section-space feature-section page-width"><div class="section-number">02 <span>${t('workflowLabel')}</span></div><div class="terminal-wrap"><div class="terminal-head"><span>~/nanh/harness</span><span>${t('workflowMode')}</span></div><div class="terminal-body"><p><span class="terminal-prompt">$</span> nanh opencode</p><p class="terminal-muted">${t('managedLaunchResult')}</p><p><span class="terminal-prompt">$</span> nanh config opencode</p><p class="terminal-muted">${t('nativeSetupResult')}</p><p><span class="terminal-prompt">$</span> opencode</p><p class="terminal-ok">${t('directLaunchResult')}</p><p class="terminal-cursor">_</p></div></div><div class="feature-copy"><h2>${t('workflowHeading')}<br><em>${t('workflowSubheading')}</em></h2><p>${t('workflowText')}</p></div></section>

    <section class="section-space feature-section telemetry-section page-width"><div class="section-number">03 <span>${t('telemetryLabel')}</span></div><div class="telemetry-panel" aria-label="${t('telemetryPanelTitle')}"><div class="telemetry-panel-visual"><div class="telemetry-art">${telemetryArt()}</div></div></div><div class="feature-copy"><h2>${t('telemetryHeading')}<br><em>${t('telemetrySubheading')}</em></h2><p>${t('telemetryText')}</p><div class="feature-command"><span>$</span><code>${t('telemetryCommand')}</code></div></div></section>

    <section class="section-space faq-section page-width" id="faq"><div class="section-number">04 <span>${t('faq')}</span></div><div class="section-heading"><h2>${faqHeading[0]}<br><em>${faqHeading[1]}</em></h2></div><div class="faq-list">${faqRows()}</div></section>

    <section class="final-cta"><h2>${t('finalAgent')}<br><em>${t('finalNan')}</em></h2><a class="text-link light-link" href="docs.html">${t('startDocs')} ${arrow()}</a></section>
  </main>${footer()}`;
}

function docsNav(section = 'docs') {
  const sectionLabel = section === 'logos' ? t('logoNotices') : t('docs');
  return `<a class="skip-link" href="#main-content">${t('skipToContent')}</a><header class="docs-header"><a class="brand-link" href="index.html" aria-label="${t('homeAria')}">${wordmark()}</a><span class="docs-label">${sectionLabel}</span><nav aria-label="${t('mainNavigation')}"><a href="index.html">${t('apps')}</a>${githubLink()}${languageSelector()}</nav></header>`;
}

function attr(value) {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function docsCode(commands) {
  const joined = commands.join('\n');
  const lines = commands.map((command) => `<b>$</b> ${attr(command)}`).join('<br>');
  return `<div class="code-block docs-code"><div><span>shell</span><button type="button" data-copy="${attr(joined)}" data-state="copy" aria-label="${t('copyCommand')}" title="${t('copyCommand')}"><svg class="copy-icon" viewBox="0 0 16 16" aria-hidden="true"><rect x="5" y="2.5" width="8" height="9" rx="1.2"></rect><path d="M3 5.5v7A1.5 1.5 0 0 0 4.5 14H10"></path></svg><svg class="check-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="m3 8.2 3.1 3.1L13 4.8"></path></svg></button></div><code>${lines}</code><small class="copy-status" role="status" aria-live="polite"></small></div>`;
}

function docsBlock([kind, value, rows]) {
  if (kind === 'p') return `<p>${value}</p>`;
  if (kind === 'h3') return `<h3>${value}</h3>`;
  if (kind === 'note') return `<div class="docs-callout">${value}</div>`;
  if (kind === 'code') return docsCode([value]);
  if (kind === 'codes') return docsCode(value);
  if (kind === 'table') {
    const head = value.map((cell) => `<th scope="col">${cell}</th>`).join('');
    const body = rows.map((row) => `<tr>${row.map((cell) => `<td>${cell}</td>`).join('')}</tr>`).join('');
    return `<div class="docs-table"><table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></div>`;
  }
  return '';
}

function docs() {
  const sections = t('docsSections');
  const started = sections.slice(0, 2);
  const reference = sections.slice(2);
  const link = ([id, title], index) => `<a href="#${id}"${index === 0 ? ' class="active"' : ''}>${title}</a>`;
  const body = sections.map(([id, title, blocks]) => `<section class="docs-section" id="${id}"><h2>${title}</h2>${blocks.map(docsBlock).join('')}</section>`).join('');
  return `${docsNav()}<div class="docs-layout"><nav class="docs-sidebar" aria-label="${t('docsNavigation')}"><p>${t('docsNavStart')}</p>${started.map(link).join('')}<p>${t('docsNavReference')}</p>${reference.map((section) => link(section, 1)).join('')}</nav><main class="docs-main" id="main-content"><nav class="docs-breadcrumb" aria-label="${t('breadcrumb')}">${t('docs')}</nav><h1>${t('docsHeading')}</h1><p class="docs-lede">${t('docsIntro')}</p>${body}</main></div>`;
}

function logos() {
  const headers = t('logosTableHeaders');
  const rows = t('logosSources').map((row) => `<tr>${row.map((cell) => `<td>${cell}</td>`).join('')}</tr>`).join('');
  return `${docsNav('logos')}<main class="docs-main logos-main" id="main-content"><nav class="docs-breadcrumb" aria-label="${t('breadcrumb')}">${t('logoNotices')}</nav><h1>${t('logosHeading')}</h1><p class="docs-lede">${t('logosIntro')}</p><section class="logos-section"><h2>${t('logosSimpleIconsHeading')}</h2><p>${t('logosSimpleIconsText')}</p><p>${t('logosPackage')}</p></section><section class="logos-section"><h2>${t('logosOfficialHeading')}</h2><p>${t('logosOfficialText')}</p><div class="logos-table"><table><thead><tr>${headers.map((cell) => `<th scope="col">${cell}</th>`).join('')}</tr></thead><tbody>${rows}</tbody></table></div><p>${t('logosLicenseText')}</p></section></main>${footer()}`;
}

const page = document.body.dataset.page;
document.body.className = page === 'docs' ? 'docs-page' : page === 'logos' ? 'logos-page' : 'landing-page';
document.getElementById('app').innerHTML = page === 'docs' ? docs() : page === 'logos' ? logos() : landing();
document.documentElement.lang = currentLocale;
document.title = page === 'docs' ? t('docsTitle') : page === 'logos' ? t('logosTitle') : t('siteTitle');
document.querySelector('meta[name="description"]').content = page === 'docs' ? t('docsMeta') : page === 'logos' ? t('logosMeta') : t('siteMeta');

const telemetryCommandBlock = document.querySelector('.feature-command');
if (telemetryCommandBlock) {
  const command = t('telemetryCommand');
  telemetryCommandBlock.classList.add('code-block');
  telemetryCommandBlock.dataset.telemetryCommand = 'true';
  telemetryCommandBlock.innerHTML = `<div class="feature-command-main"><span>$</span><code>${command}</code><button type="button" data-copy="${command}" data-state="copy" aria-label="${t('copyCommand')}" title="${t('copyCommand')}"><svg class="copy-icon" viewBox="0 0 16 16" aria-hidden="true"><rect x="5" y="2.5" width="8" height="9" rx="1.2"></rect><path d="M3 5.5v7A1.5 1.5 0 0 0 4.5 14H10"></path></svg><svg class="check-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="m3 8.2 3.1 3.1L13 4.8"></path></svg></button></div><small class="copy-status" role="status" aria-live="polite"></small>`;
}

registerWebMcpTools();


// interactions.js owns the IntersectionObserver and prefers-reduced-motion: reduce
// behavior while receiving only this temporary dependency bridge.
window.nanHarness = { harnesses, t, installTargetCommand, writeClipboard };
