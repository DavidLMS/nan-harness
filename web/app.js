const harnesses = [
  ['claude', 'Claude Code', 'nan claude', 'C', 'logos/claude.svg'],
  ['codex', 'Codex', 'nan codex', '>_', 'logos/codex.png'],
  ['opencode', 'OpenCode', 'nan opencode', '□', 'logos/opencode.svg'],
  ['hermes', 'Hermes', 'nan hermes', 'H', 'logos/hermes.png'],
  ['omp', 'Oh My Pi', 'nan omp', 'π', 'logos/omp.svg'],
  ['pi', 'Pi', 'nan pi', 'π', 'logos/pi.svg'],
  ['prime', 'Prime Agent', 'nan prime-agent', 'P', 'logos/prime.svg'],
  ['deepseek', 'DeepSeek', 'nan dsh', 'D', 'logos/deepseek.svg'],
  ['openclaw', 'OpenClaw', 'nan openclaw', '◈', 'logos/openclaw.svg'],
  ['cline', 'Cline', 'nan cline', 'CL', 'logos/cline.svg'],
  ['qwen', 'Qwen Code', 'nan qwen', 'Q', 'logos/qwen.svg'],
  ['kimi', 'Kimi Code', 'nan kimi', 'K', 'logos/kimi.svg'],
  ['aider', 'Aider', 'nan aider', 'A', 'logos/aider.svg'],
  ['goose', 'Goose', 'nan goose', 'G', 'logos/goose.svg'],
  ['fx', 'fx', 'nan fx', 'fx', 'logos/fx.svg']
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

const translations = {
  en: {
    siteTitle: 'nan-harness — one command to rule all harnesses, one command to bind them',
    siteMeta: 'nan-harness — use NaN models from the coding agents you already use.',
    docsTitle: 'nan-harness — docs',
    docsMeta: 'Documentation for nan-harness.',
    logosTitle: 'nan-harness — logo notices',
    logosMeta: 'Logo notices for nan-harness.',
    language: 'Language',
    homeAria: 'nan-harness home',
    mainNavigation: 'Main navigation',
    docsNavigation: 'Documentation navigation',
    breadcrumb: 'Breadcrumb',
    skipToContent: 'Skip to content',
    docs: 'DOCS',
    faq: 'FAQ',
    getStarted: 'GET STARTED',
    githubAria: 'Open nan-harness on GitHub',
    readDocs: 'READ THE DOCS',
    seeHarnesses: 'SEE THE HARNESSES',
    heroTitle: 'ALL|HARNESSES|WITH NAN.',
    heroLede: `Run any supported coding agent with ${nanLink('NaN')} through nan-harness. One command keeps models, credentials and compatibility checks current—without persistent provider configuration or a new interface to learn.`,
    whatIs: 'WHAT IS NAN-HARNESS',
    oneLocal: 'The model moves.',
    everyAgent: 'Your workflow stays.',
    whatText: 'NaN gives you the models. Your agent gives you the loop. nan-harness connects them with a managed NaN route that works across every supported harness. It keeps compatibility checks, provider routing and optional error reporting in one place while you keep the original client and workflows.',
    howItWorks: 'HOW IT WORKS',
    workflowLabel: 'THE RECOMMENDED WAY',
    discovering: '→ discovering harness',
    startingRoute: '→ starting local route',
    ready: '→ harness ready',
    workflowHeading: 'Run it through nan-harness.',
    workflowSubheading: 'One command.',
    workflowText: 'Use <code>nan &lt;harness&gt;</code> for every supported harness. On every launch, nan-harness checks compatibility, uses your current models and credentials, prepares any required bridges and supervises the process. If you do not want nan-harness to manage the process and simply want to configure the harness natively with NaN models, you can use <code>nan config &lt;harness&gt;</code>. But you will need to update that configuration if anything changes in the future.',
    workflowMode: 'recommended / advanced',
    managedLaunchResult: '→ recommended: checks, routes and supervises OpenCode',
    nativeSetupResult: '→ advanced: writes persistent OpenCode configuration',
    directLaunchResult: '→ requires copied credentials and catalogue maintenance',
    telemetryLabel: 'PRIVACY BY DESIGN',
    telemetryHeading: 'Telemetry off.',
    telemetrySubheading: 'Consent on.',
    telemetryCommand: 'nan telemetry on',
    telemetryText: 'Opting in enables sanitized error reports and sends a minimal invocation event with a random installation identifier, the nan-harness version, harness, operation, transport, OS family, architecture, and target environment. It never collects prompts, output, arguments, paths, models, credentials, usernames, or hostnames.',
    telemetryPanelTitle: 'nan telemetry',
    faqHeading: 'WORTH|KNOWING.',
    finalAgent: 'EVERY AGENT.',
    finalNan: 'NAN-ROUTED.',
    startDocs: 'START WITH THE DOCS',
    installLatest: 'INSTALL LATEST RELEASE',
    installCommand: unixInstallCommand,
    installWindowsCommand: windowsInstallCommand,
    installPlatform: 'Choose your operating system',
    unixTab: 'macOS / LINUX',
    windowsTab: 'WINDOWS',
    copy: 'Copy',
    chooseHarness: 'Choose a harness',
    oneRoute: 'ONE ROUTE · EVERY HARNESS',
    copyCommand: 'Copy command',
    copyingCommand: 'Copying command',
    commandCopied: 'Command copied',
    pauseCarousel: 'Pause automatic selection',
    resumeCarousel: 'Resume automatic selection',
    footerTagline: 'Every harness. Your favourite provider.',
    logoNotices: 'Logo notices',
    logosHeading: 'Harness logo notices',
    logosIntro: 'These assets identify supported third-party products. Their inclusion does not imply sponsorship, endorsement, or affiliation. Names and trademarks remain the property of their respective owners and are not covered by this repository\'s Apache-2.0 license.',
    logosSimpleIconsHeading: 'Simple Icons 16.28.0',
    logosSimpleIconsText: '<code>claude.svg</code>, <code>opencode.svg</code>, <code>pi.svg</code>, <code>deepseek.svg</code>, <code>cline.svg</code>, <code>qwen.svg</code>, and <code>kimi.svg</code> come from the <code>simple-icons</code> npm package version 16.28.0. Simple Icons releases its icon data under CC0-1.0. Its license and trademark disclaimer are included in <a href="logos/licenses/CC0-1.0.txt">licenses/CC0-1.0.txt</a> and <a href="logos/licenses/SIMPLE-ICONS-DISCLAIMER.md">licenses/SIMPLE-ICONS-DISCLAIMER.md</a>.',
    logosPackage: 'Package: <a href="https://www.npmjs.com/package/simple-icons/v/16.28.0" target="_blank" rel="noreferrer">simple-icons 16.28.0 on npm</a>',
    logosOfficialHeading: 'Official project sources',
    logosOfficialText: 'The remaining marks come from the official project sources below. They are included to identify the supported products.',
    logosTableHeaders: ['Asset', 'Source', 'Terms'],
    logosSources: [
      ['codex.png', '<a href="https://marketplace.visualstudio.com/items?itemName=OpenAI.chatgpt" target="_blank" rel="noreferrer">Official Codex VS Code extension 26.5818.31338</a>, using its <a href="https://openai.gallerycdn.vsassets.io/extensions/openai/chatgpt/26.5818.31338/1787264961823/Microsoft.VisualStudio.Services.Icons.Default" target="_blank" rel="noreferrer">published icon</a>', '<a href="https://openai.com/brand/" target="_blank" rel="noreferrer">OpenAI brand guidelines</a>; black background converted to transparency without changing the mark geometry'],
      ['hermes.png', '<a href="https://github.com/NousResearch/hermes-agent/blob/06b9141109fbd320b14b8c88645ab37fc4f42c9d/apps/desktop/assets/icon.png" target="_blank" rel="noreferrer"><code>NousResearch/hermes-agent</code> at <code>06b9141</code></a>', 'MIT; extracted from the locally installed desktop app and resized to 256 px without visual changes'],
      ['omp.svg', 'The official <a href="https://omp.sh/" target="_blank" rel="noreferrer"><code>omp.sh</code> header mark</a>, retrieved 2026-08-31', 'Used only for product identification; copied from the published header SVG without changing its geometry or colours'],
      ['prime.svg', '<a href="https://github.com/PrimeIntellect-ai/prime-agent/blob/c75a637b00d3b52762841e72efe92289a0d55b49/assets/brand/prime-butterfly.svg" target="_blank" rel="noreferrer"><code>PrimeIntellect-ai/prime-agent</code> at <code>c75a637</code></a>', 'MIT'],
      ['openclaw.svg', '<a href="https://github.com/openclaw/openclaw/blob/73ff2d2f45b26856882423d9ae87a76a727ac7cd/ui/public/favicon.svg" target="_blank" rel="noreferrer"><code>openclaw/openclaw</code> at <code>73ff2d2</code></a>', 'MIT'],
      ['aider.svg', '<a href="https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/website/assets/logo.svg" target="_blank" rel="noreferrer"><code>Aider-AI/aider</code> at <code>5dc9490</code></a>', 'Apache-2.0'],
      ['goose.svg', '<a href="https://github.com/aaif-goose/goose/blob/384b6cc0d3b52762841e72efe92289a0d55b49/documentation/static/img/goose.svg" target="_blank" rel="noreferrer"><code>aaif-goose/goose</code> at <code>384b6cc</code></a>', 'Apache-2.0'],
      ['fx.svg', 'The official <a href="https://fx.sh/" target="_blank" rel="noreferrer"><code>fx.sh</code> header mark</a>, associated with <a href="https://github.com/vercel-labs/fx/tree/580a0c5da9386319741968c09c1cee69e763487a" target="_blank" rel="noreferrer"><code>vercel-labs/fx</code> at <code>580a0c5</code></a>', 'Used only for product identification; the linked project repository is Apache-2.0; static glyph extracted without the website-only hover shimmer']
    ],
    logosLicenseText: 'The Apache-2.0 text is included in <a href="logos/licenses/APACHE-2.0.txt">licenses/APACHE-2.0.txt</a>. The applicable MIT notices are included as separate files: <a href="logos/licenses/MIT-hermes-agent.txt">MIT-hermes-agent.txt</a>, <a href="logos/licenses/MIT-openclaw.txt">MIT-openclaw.txt</a>, and <a href="logos/licenses/MIT-prime-agent.txt">MIT-prime-agent.txt</a>. The Codex mark is used only to identify the supported OpenAI product and remains subject to OpenAI\'s brand guidelines.',
    searchDocs: 'Search docs...',
    docsHeading: 'nan-harness',
    docsIntro: `Run the coding agents you already know with ${nanLink('NaN')} through nan-harness. <code>nan &lt;agent&gt;</code> is the recommended command for everyday use; <code>nan config &lt;agent&gt;</code> is an advanced native-setup option. <code>nan-harness</code> is the full command and <code>nan</code> is its shorter alias, so the examples use it.`,
    docsNavStart: 'GET STARTED',
    docsNavReference: 'REFERENCE',
    docsSections: [
      ['install', 'INSTALL', [
        ['p', 'One line in your terminal. On macOS and Linux:'],
        ['code', unixInstallCommand],
        ['p', 'On Windows, in PowerShell:'],
        ['code', windowsInstallCommand],
        ['p', 'If it asks you to open a new terminal, open it. Then make sure it is there:'],
        ['code', 'nan --version'],
        ['p', 'The installer provides both commands: use <code>nan-harness</code> when you want the full project name, or the shorter <code>nan</code> alias in everyday commands. There are builds for macOS, Linux and Windows, and if you would rather compile it yourself the instructions are in the <a href="https://github.com/DavidLMS/nan-harness">repository</a>.']
      ]],
      ['first-run', 'FIRST RUN', [
        ['p', 'Go to your project and launch the agent you already use:'],
        ['code', 'nan claude'],
        ['p', 'The first time, unless you already have it in your environment, nan-harness asks for your NaN API key, checks that it works and saves it. It will not ask again.'],
        ['p', 'From there on it is the same agent as always. To pick a model as it starts, use <code>--model</code>. Where the agent supports it, you can also use its native model picker:'],
        ['codes', ['nan codex --model qwen3.6', 'nan opencode --model deepseek-v4-flash']],
        ['p', 'Whatever you want to hand to the agent itself goes after <code>--</code>, untouched:'],
        ['codes', ['nan codex --model qwen3.6 -- --full-auto', 'nan claude -- --resume']],
        ['h3', 'Your key'],
        ['note', '<strong>If you already have <code>NAN_API_KEY</code> in your environment, this section does not apply to you.</strong> That key wins over any other: nan-harness will not ask you for one, saves nothing to disk, and there is no need to log in.'],
        ['p', 'If you do not, nan-harness keeps it where your system keeps passwords: Keychain on macOS, Credential Manager on Windows, Secret Service on Linux. If none of them is available it uses a private file and warns you.'],
        ['table', ['Command', 'What it is for'], [
          ['nan auth login', 'Enter a key, or replace the one you have.'],
          ['nan auth status', 'See which key is in use and where it is stored.'],
          ['nan auth logout', 'Delete the saved key.']
        ]],
        ['p', 'The environment variable is the usual route on servers and in CI, where there is nobody to type it. <code>nan auth status</code> will tell you at any point where the key in use comes from:'],
        ['code', 'export NAN_API_KEY="<your-NaN-api-key>"'],
        ['note', '<strong>The key is yours alone.</strong> Keep it out of commits, logs and bug reports. nan-harness never takes it as a command-line argument, so it cannot end up in your shell history.']
      ]],
      ['harnesses', 'HARNESSES', [
        ['p', '<code>nan &lt;agent&gt;</code> is the recommended workflow for every supported agent. nan-harness checks the installed version, reads the current NaN catalogue, prepares any required bridge and supervises the process without changing persistent provider settings. <code>nan config &lt;agent&gt;</code> is an advanced option for agents that support direct native configuration.'],
        ['table', ['Recommended command', 'Agent', 'Native setup'], [
          ['nan aider', harnessLink('Aider', 'aider'), 'optional'],
          ['nan cline', harnessLink('Cline', 'cline'), 'optional'],
          ['nan goose', harnessLink('Goose', 'goose'), 'optional'],
          ['nan claude', harnessLink('Claude Code', 'claude'), 'not available'],
          ['nan codex', harnessLink('Codex', 'codex'), 'not available'],
          ['nan opencode', harnessLink('OpenCode', 'opencode'), 'optional'],
          ['nan qwen', harnessLink('Qwen Code', 'qwen'), 'optional'],
          ['nan pi', harnessLink('Pi', 'pi'), 'optional'],
          ['nan kimi', harnessLink('Kimi Code', 'kimi'), 'optional'],
          ['nan openclaw', harnessLink('OpenClaw', 'openclaw'), 'optional'],
          ['nan hermes', harnessLink('Hermes Agent', 'hermes'), 'optional'],
          ['nan omp', harnessLink('Oh My Pi', 'omp'), 'optional'],
          ['nan prime-agent', harnessLink('Prime Agent', 'prime'), 'optional'],
          ['nan dsh', harnessLink('DeepSeek Harness', 'deepseek'), 'optional'],
          ['nan fx', harnessLink('fx', 'fx'), 'not available']
        ]],
        ['h3', 'Recommended: run with nan-harness'],
        ['p', 'For everyday use, run <code>nan &lt;agent&gt;</code>. It checks the installed version, discovers your current NaN models, uses your current credential source, prepares any required bridge and supervises the agent process. If the agent is missing, nan-harness can offer to install it. When telemetry is enabled, it can also send sanitized reports for CLI and bridge failures.'],
        ['code', 'nan opencode'],
        ['h3', 'Advanced: native setup'],
        ['p', 'Use <code>nan config &lt;agent&gt;</code> only when another tool or integration needs to start a supported agent with its usual executable. Native setup writes persistent provider settings; the later direct launch runs outside nan-harness\'s compatibility and observability layer. This command only configures the agent; start it afterwards with its usual executable:'],
        ['codes', ['nan config opencode', 'opencode']],
        ['p', 'Claude Code, Codex and fx need nan-harness running because their NaN connection depends on a local bridge or gateway. They cannot be prepared for standalone use with <code>nan config</code>.'],
        ['p', 'Native setup uses the API key saved by <code>nan auth login</code>; an environment-only key is never copied into another application. Use <code>--status</code> to inspect it and <code>--refresh</code> after changing your saved key or model catalogue. <code>--remove</code> removes what nan-harness added and restores previous settings when it is safe to do so.']
      ]],
      ['options', 'OPTIONS', [
        ['h3', 'Recommended launch options'],
        ['table', ['Option', 'What it does'], [
          ['--model &lt;id&gt;', 'Which model to use this time.'],
          ['--allow-untested', 'Runs an agent version newer than the ones tested with this release of nan-harness.'],
          ['--allow-unsupported', 'Runs a version that is too old, or one whose version nan-harness cannot read.']
        ]],
        ['h3', 'Advanced native setup commands'],
        ['table', ['Command', 'What it does'], [
          ['nan config &lt;agent&gt;', 'Writes a reversible native NaN configuration without launching the agent.'],
          ['nan config &lt;agent&gt; --status', 'Checks whether its managed configuration is current.'],
          ['nan config &lt;agent&gt; --refresh', 'Updates the copied key, model catalogue and managed defaults.'],
          ['nan config &lt;agent&gt; --remove', 'Removes the managed configuration and restores safe previous values.']
        ]],
        ['h3', 'The rest of the commands'],
        ['table', ['Command', 'What it does'], [
          ['nan doctor', 'Checks everything and tells you how it looks.'],
          ['nan doctor &lt;agent&gt;', 'Checks one agent in detail.'],
          ['nan auth login', 'Saves your NaN key.'],
          ['nan auth status', 'Tells you which key is in use.'],
          ['nan auth logout', 'Deletes the saved key and lets you remove configurations that contain a copy.'],
          ['nan config --status', 'Shows every native configuration managed by nan-harness.'],
          ['nan config --refresh-all', 'Refreshes every managed native configuration.'],
          ['nan config --remove-all', 'Removes every managed native configuration.'],
          ['nan update', 'Updates nan-harness to the latest version.'],
          ['nan telemetry on|off', 'Turns anonymous telemetry on or off.'],
          ['nan uninstall', 'Removes nan-harness and everything it left behind.'],
          ['nan --help', 'The full list, in your terminal.']
        ]]
      ]],
      ['help', 'HELP AND PRIVACY', [
        ['h3', 'If something does not work'],
        ['p', 'Run <code>nan doctor</code>. It tells you whether it can reach NaN, how many models you have available, which agents you have installed and which ones are out of date.'],
        ['code', 'nan doctor'],
        ['p', 'That report is safe to share: no keys, no paths, nothing you typed or the model answered. The single-agent version does show where the program lives on your machine, so give it a read first.'],
        ['code', 'nan doctor claude'],
        ['h3', 'Version warnings'],
        ['p', 'Each release of nan-harness is tested against specific versions of each agent. If yours is newer, you get a warning and it runs anyway. If it is older than supported, or nan-harness cannot read its version, it stops and lets you decide with <code>--allow-untested</code> or <code>--allow-unsupported</code>.'],
        ['h3', 'Telemetry'],
        ['p', 'Off unless you turn it on. If enabled, nan-harness can send sanitized CLI and bridge error reports plus a minimal invocation event with a random installation identifier, its version, the harness, operation, transport, OS family, architecture, and target environment. It never sends prompts, output, arguments, paths, models, credentials, usernames, or hostnames. As with any HTTPS request, the receiving infrastructure can observe ordinary network metadata.'],
        ['codes', ['nan telemetry on', 'nan telemetry off']],
        ['p', 'It helps us see which agents need the most attention. Turning it off stops the events and deletes the anonymous identifier that counted them.'],
        ['h3', 'Uninstalling'],
        ['p', 'It asks for confirmation, undoes every configuration it left in your agents, deletes your saved key and removes itself. If you changed one of those configurations by hand, it stops instead of overwriting your work.'],
        ['code', 'nan uninstall']
      ]]
    ],
    guides: 'GUIDES',
    cliReference: 'CLI REFERENCE',
    getStartedSection: 'GET STARTED',
    introduction: 'INTRODUCTION',
    gettingStarted: 'GETTING STARTED',
    examples: 'EXAMPLES',
    agents: 'AGENTS',
    apps: 'LANDING',
    welcome: 'Welcome to nan-harness.',
    docsLede: "This documentation explains the recommended way to run your coding agents with NaN through nan-harness, plus the advanced native-setup option for supported agents.",
    callout: '<strong>To get started</strong> Make sure your NaN API key is available locally. The key is personal and stays in your environment.',
    gettingStartedText: 'Start with <code>nan &lt;agent&gt;</code> so nan-harness can check compatibility, prepare the connection and supervise the agent. Use native setup only when you specifically need to start a compatible agent with its own command.',
    examplesText: 'The recommended workflow keeps provider routing and maintenance in nan-harness. Advanced native setup writes persistent settings into a compatible agent.',
    whatNext: 'What to do next',
    next: 'NEXT',
    copied: 'Copied ',
    copiedStatus: 'Copied to clipboard.',
    telemetryCopiedStatus: 'Copied to clipboard. Thanks for helping improve nan-harness.',
    copyFailed: 'Copy failed. Try again.',
    faqs: [
      ['What is nan-harness?', `The recommended way to use any supported harness with the models available through your ${nanLink('NaN community subscription')}. The project command is <code>nan-harness</code>; <code>nan</code> is its short alias.`],
      ['Does it replace my agent?', 'No. You keep the original agent, profile and workflows. nan-harness manages its launch and NaN connection without replacing the client or writing persistent provider settings.'],
      ['When should I use nan config?', 'Only when you specifically need to start a supported agent with its normal command. For everyday use, prefer <code>nan &lt;agent&gt;</code>: it works across every supported agent, reads the current model catalogue, avoids persistent provider changes, checks compatibility and can send sanitized error reports when telemetry is enabled. Native setup is unavailable for Claude Code, Codex and fx and must be refreshed when your saved key or model catalogue changes.'],
      ['How are models discovered?', '<code>nan &lt;agent&gt;</code> reads the current NaN catalogue at launch. Advanced native setup writes a catalogue snapshot into the agent, so you must run <code>nan config &lt;agent&gt; --refresh</code> when your available models change.'],
      ['Can I request another harness?', 'Yes. If a harness you use is missing, <a href="https://github.com/DavidLMS/nan-harness/issues/new" target="_blank" rel="noreferrer">open an issue</a> and tell us which one you would like to see supported.']
    ]
  },
  es: {
    siteTitle: 'nan-harness — todos los harness con NaN',
    siteMeta: 'nan-harness — usa modelos de NaN con los agentes de código que ya utilizas.',
    docsTitle: 'nan-harness — documentación',
    docsMeta: 'Documentación de nan-harness.',
    logosTitle: 'nan-harness — aviso sobre logos',
    logosMeta: 'Aviso sobre logos de nan-harness.',
    language: 'Idioma',
    homeAria: 'Inicio de nan-harness',
    mainNavigation: 'Navegación principal',
    docsNavigation: 'Navegación de la documentación',
    breadcrumb: 'Ruta de navegación',
    skipToContent: 'Saltar al contenido',
    docs: 'DOCUMENTACIÓN',
    faq: 'FAQ',
    getStarted: 'EMPEZAR',
    githubAria: 'Abrir nan-harness en GitHub',
    readDocs: 'Documentación',
    seeHarnesses: 'VER LOS HARNESSES',
    heroTitle: 'TODOS|LOS HARNESSES|CON NAN.',
    heroLede: `Ejecuta cualquier agente de código compatible con ${nanLink('NaN')} mediante nan-harness. Un solo comando mantiene al día los modelos, las credenciales y las comprobaciones de compatibilidad, sin configuración persistente del proveedor ni otra interfaz que aprender.`,
    whatIs: 'QUÉ ES NAN-HARNESS',
    oneLocal: 'El modelo se mueve.',
    everyAgent: 'Tu flujo se mantiene.',
    whatText: 'NaN te da los modelos. Tu agente te da el loop. nan-harness los conecta mediante una ruta gestionada a NaN que funciona con todos los harness compatibles. Mantiene en un solo lugar las comprobaciones de compatibilidad, el enrutamiento del proveedor y los informes de error opcionales, mientras tú conservas el cliente y sus flujos originales.',
    howItWorks: 'CÓMO FUNCIONA',
    workflowLabel: 'LA FORMA RECOMENDADA',
    discovering: '→ descubriendo harness',
    startingRoute: '→ iniciando ruta local',
    ready: '→ harness listo',
    workflowHeading: 'Arráncalo con nan-harness.',
    workflowSubheading: 'Un comando.',
    workflowText: 'Usa <code>nan &lt;harness&gt;</code> con cualquier harness compatible. En cada arranque, comprueba la compatibilidad, utiliza tus modelos y credenciales actuales, prepara los bridges necesarios y supervisa el proceso. Si no quieres que nan-harness gestione el proceso y simplemente quieres realizar la configuración nativa del harness con los modelos de NaN, puedes usar <code>nan config &lt;harness&gt;</code>. Pero tendrás que actualizarla si hay cambios en el futuro.',
    workflowMode: 'recomendado / avanzado',
    managedLaunchResult: '→ recomendado: comprueba, conecta y supervisa OpenCode',
    nativeSetupResult: '→ avanzado: escribe configuración persistente en OpenCode',
    directLaunchResult: '→ requiere mantener la credencial y el catálogo copiados',
    telemetryLabel: 'PRIVACIDAD POR DISEÑO',
    telemetryHeading: 'Telemetría apagada.',
    telemetrySubheading: 'Consentimiento activo.',
    telemetryCommand: 'nan telemetry on',
    telemetryText: 'Activarla permite enviar informes de error sanitizados y un evento mínimo de uso con un identificador aleatorio de instalación, la versión de nan-harness, el harness, la operación, el transporte, la familia del sistema operativo, la arquitectura y el entorno de destino. Nunca recoge prompts, output, argumentos, rutas, modelos, credenciales, nombres de usuario ni nombres de equipo.',
    telemetryPanelTitle: 'nan telemetry',
    faqHeading: 'CONVIENE|SABER.',
    finalAgent: 'CADA AGENTE.',
    finalNan: 'CON RUTA NAN.',
    startDocs: 'Comienza leyendo la documentación',
    installLatest: 'INSTALAR ÚLTIMA VERSIÓN',
    installCommand: unixInstallCommand,
    installWindowsCommand: windowsInstallCommand,
    installPlatform: 'Elige tu sistema operativo',
    unixTab: 'macOS / LINUX',
    windowsTab: 'WINDOWS',
    copy: 'Copiar',
    chooseHarness: 'Elige un harness',
    oneRoute: 'UNA RUTA · CADA HARNESS',
    copyCommand: 'Copiar comando',
    copyingCommand: 'Copiando comando',
    commandCopied: 'Comando copiado',
    pauseCarousel: 'Pausar selección automática',
    resumeCarousel: 'Reanudar selección automática',
    footerTagline: 'Todos los harness con tu proveedor favorito',
    logoNotices: 'Aviso sobre logos',
    logosHeading: 'Aviso sobre logos',
    logosIntro: 'Estos recursos identifican productos de terceros compatibles. Su inclusión no implica patrocinio, respaldo ni relación alguna. Los nombres y las marcas registradas siguen siendo propiedad de sus respectivos titulares y no están cubiertos por la licencia Apache-2.0 de este repositorio.',
    logosSimpleIconsHeading: 'Simple Icons 16.28.0',
    logosSimpleIconsText: '<code>claude.svg</code>, <code>opencode.svg</code>, <code>pi.svg</code>, <code>deepseek.svg</code>, <code>cline.svg</code>, <code>qwen.svg</code> y <code>kimi.svg</code> proceden del paquete npm <code>simple-icons</code> versión 16.28.0. Simple Icons publica los datos de sus iconos bajo CC0-1.0. La licencia y el aviso sobre marcas están incluidos en <a href="logos/licenses/CC0-1.0.txt">licenses/CC0-1.0.txt</a> y <a href="logos/licenses/SIMPLE-ICONS-DISCLAIMER.md">licenses/SIMPLE-ICONS-DISCLAIMER.md</a>.',
    logosPackage: 'Paquete: <a href="https://www.npmjs.com/package/simple-icons/v/16.28.0" target="_blank" rel="noreferrer">simple-icons 16.28.0 en npm</a>',
    logosOfficialHeading: 'Fuentes oficiales de los proyectos',
    logosOfficialText: 'El resto de las marcas procede de las fuentes oficiales de cada proyecto que aparecen a continuación. Se incluyen únicamente para identificar los productos compatibles.',
    logosTableHeaders: ['Recurso', 'Fuente', 'Términos'],
    logosSources: [
      ['codex.png', '<a href="https://marketplace.visualstudio.com/items?itemName=OpenAI.chatgpt" target="_blank" rel="noreferrer">Extensión oficial de Codex para VS Code 26.5818.31338</a>, usando su <a href="https://openai.gallerycdn.vsassets.io/extensions/openai/chatgpt/26.5818.31338/1787264961823/Microsoft.VisualStudio.Services.Icons.Default" target="_blank" rel="noreferrer">icono publicado</a>', '<a href="https://openai.com/brand/" target="_blank" rel="noreferrer">Directrices de marca de OpenAI</a>; el fondo negro se convirtió en transparente sin cambiar la geometría de la marca'],
      ['hermes.png', '<a href="https://github.com/NousResearch/hermes-agent/blob/06b9141109fbd320b14b8c88645ab37fc4f42c9d/apps/desktop/assets/icon.png" target="_blank" rel="noreferrer"><code>NousResearch/hermes-agent</code> en <code>06b9141</code></a>', 'MIT; extraído de la aplicación de escritorio instalada localmente y redimensionado a 256 px sin cambios visuales'],
      ['omp.svg', 'La marca de la cabecera oficial de <a href="https://omp.sh/" target="_blank" rel="noreferrer"><code>omp.sh</code></a>, recuperada el 2026-08-31', 'Usada únicamente para identificar el producto; copiada del SVG publicado en la cabecera sin cambiar su geometría ni sus colores'],
      ['prime.svg', '<a href="https://github.com/PrimeIntellect-ai/prime-agent/blob/c75a637b00d3b52762841e72efe92289a0d55b49/assets/brand/prime-butterfly.svg" target="_blank" rel="noreferrer"><code>PrimeIntellect-ai/prime-agent</code> en <code>c75a637</code></a>', 'MIT'],
      ['openclaw.svg', '<a href="https://github.com/openclaw/openclaw/blob/73ff2d2f45b26856882423d9ae87a76a727ac7cd/ui/public/favicon.svg" target="_blank" rel="noreferrer"><code>openclaw/openclaw</code> en <code>73ff2d2</code></a>', 'MIT'],
      ['aider.svg', '<a href="https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/website/assets/logo.svg" target="_blank" rel="noreferrer"><code>Aider-AI/aider</code> en <code>5dc9490</code></a>', 'Apache-2.0'],
      ['goose.svg', '<a href="https://github.com/aaif-goose/goose/blob/384b6cc0d3b52762841e72efe92289a0d55b49/documentation/static/img/goose.svg" target="_blank" rel="noreferrer"><code>aaif-goose/goose</code> en <code>384b6cc</code></a>', 'Apache-2.0'],
      ['fx.svg', 'La <a href="https://fx.sh/" target="_blank" rel="noreferrer">marca de cabecera oficial de <code>fx.sh</code></a>, asociada a <a href="https://github.com/vercel-labs/fx/tree/580a0c5da9386319741968c09c1cee69e763487a" target="_blank" rel="noreferrer"><code>vercel-labs/fx</code> en <code>580a0c5</code></a>', 'Se usa únicamente para identificar el producto; el repositorio enlazado del proyecto está bajo Apache-2.0; glifo estático extraído sin el efecto de brillo exclusivo de la web'],
    ],
    logosLicenseText: 'El texto de Apache-2.0 está incluido en <a href="logos/licenses/APACHE-2.0.txt">licenses/APACHE-2.0.txt</a>. Los avisos MIT aplicables están incluidos en archivos separados: <a href="logos/licenses/MIT-hermes-agent.txt">MIT-hermes-agent.txt</a>, <a href="logos/licenses/MIT-openclaw.txt">MIT-openclaw.txt</a> y <a href="logos/licenses/MIT-prime-agent.txt">MIT-prime-agent.txt</a>. La marca de Codex se usa únicamente para identificar el producto de OpenAI compatible y sigue sujeta a las directrices de marca de OpenAI.',
    searchDocs: 'Buscar en la documentación...',
    docsHeading: 'nan-harness',
    docsIntro: `Ejecuta mediante nan-harness los agentes de código que ya conoces con ${nanLink('NaN')}. <code>nan &lt;agente&gt;</code> es el comando recomendado para el uso diario; <code>nan config &lt;agente&gt;</code> es una opción avanzada de configuración nativa. <code>nan-harness</code> es el comando completo y <code>nan</code> es su alias corto, por eso los ejemplos lo utilizan.`,
    docsNavStart: 'EMPEZAR',
    docsNavReference: 'REFERENCIA',
    docsSections: [
      ['install', 'INSTALACIÓN', [
        ['p', 'Una línea en tu terminal. En macOS y Linux:'],
        ['code', unixInstallCommand],
        ['p', 'En Windows, desde PowerShell:'],
        ['code', windowsInstallCommand],
        ['p', 'Si te pide abrir una terminal nueva, ábrela. Después comprueba que está:'],
        ['code', 'nan --version'],
        ['p', 'El instalador proporciona los dos comandos: usa <code>nan-harness</code> cuando quieras el nombre completo del proyecto, o el alias corto <code>nan</code> en el día a día. Hay versiones para macOS, Linux y Windows, y si prefieres compilarlo por tu cuenta tienes las instrucciones en el <a href="https://github.com/DavidLMS/nan-harness">repositorio</a>.']
      ]],
      ['first-run', 'PRIMER USO', [
        ['p', 'Ve a tu proyecto y lanza el agente que ya usas:'],
        ['code', 'nan claude'],
        ['p', 'La primera vez, si no la tienes ya en tu entorno, nan-harness te pide tu API key de NaN, comprueba que funciona y la guarda. No te la volverá a pedir.'],
        ['p', 'A partir de ahí es el agente de siempre. Para elegir modelo al arrancar, usa <code>--model</code>. Cuando el agente lo permite, también puedes usar su selector nativo de modelos:'],
        ['codes', ['nan codex --model qwen3.6', 'nan opencode --model deepseek-v4-flash']],
        ['p', 'Lo que quieras pasarle al propio agente va después de <code>--</code>, tal cual:'],
        ['codes', ['nan codex --model qwen3.6 -- --full-auto', 'nan claude -- --resume']],
        ['h3', 'Tu clave'],
        ['note', '<strong>Si ya tienes <code>NAN_API_KEY</code> en tu entorno, este apartado no va contigo.</strong> Esa clave manda sobre cualquier otra: nan-harness no te pedirá ninguna, no guarda nada en disco y no necesitas hacer login.'],
        ['p', 'Si no la tienes, nan-harness la guarda donde tu sistema guarda las contraseñas: Llavero en macOS, Administrador de credenciales en Windows, Secret Service en Linux. Si no hay ninguno disponible, usa un archivo privado y te avisa.'],
        ['table', ['Comando', 'Para qué sirve'], [
          ['nan auth login', 'Introducir una clave, o cambiar la que tienes.'],
          ['nan auth status', 'Ver qué clave se está usando y dónde está guardada.'],
          ['nan auth logout', 'Borrar la clave guardada.']
        ]],
        ['p', 'La variable de entorno es la vía habitual en servidores y en CI, donde no hay nadie para escribirla. <code>nan auth status</code> te confirma en cualquier momento de dónde sale la clave que se está usando:'],
        ['code', 'export NAN_API_KEY="<tu-api-key-de-NaN>"'],
        ['note', '<strong>La clave es sólo tuya.</strong> Mantenla fuera de commits, logs e informes de error. nan-harness nunca la acepta como argumento del comando, así que no puede acabar en tu historial.']
      ]],
      ['harnesses', 'AGENTES', [
        ['p', '<code>nan &lt;agente&gt;</code> es la forma recomendada para todos los agentes compatibles. nan-harness comprueba la versión instalada, consulta el catálogo actual de NaN, prepara los bridges necesarios y supervisa el proceso sin cambiar ajustes persistentes del proveedor. <code>nan config &lt;agente&gt;</code> es una opción avanzada para los agentes que admiten configuración nativa directa.'],
        ['table', ['Comando recomendado', 'Agente', 'Configuración nativa'], [
          ['nan aider', harnessLink('Aider', 'aider'), 'opcional'],
          ['nan cline', harnessLink('Cline', 'cline'), 'opcional'],
          ['nan goose', harnessLink('Goose', 'goose'), 'opcional'],
          ['nan claude', harnessLink('Claude Code', 'claude'), 'no disponible'],
          ['nan codex', harnessLink('Codex', 'codex'), 'no disponible'],
          ['nan opencode', harnessLink('OpenCode', 'opencode'), 'opcional'],
          ['nan qwen', harnessLink('Qwen Code', 'qwen'), 'opcional'],
          ['nan pi', harnessLink('Pi', 'pi'), 'opcional'],
          ['nan kimi', harnessLink('Kimi Code', 'kimi'), 'opcional'],
          ['nan openclaw', harnessLink('OpenClaw', 'openclaw'), 'opcional'],
          ['nan hermes', harnessLink('Hermes Agent', 'hermes'), 'opcional'],
          ['nan omp', harnessLink('Oh My Pi', 'omp'), 'opcional'],
          ['nan prime-agent', harnessLink('Prime Agent', 'prime'), 'opcional'],
          ['nan dsh', harnessLink('DeepSeek Harness', 'deepseek'), 'opcional'],
          ['nan fx', harnessLink('fx', 'fx'), 'no disponible']
        ]],
        ['h3', 'Recomendado: ejecutar con nan-harness'],
        ['p', 'Para el uso diario, ejecuta <code>nan &lt;agente&gt;</code>. nan-harness comprueba la versión instalada, descubre tus modelos actuales de NaN, utiliza la fuente de credenciales activa, prepara los bridges necesarios y supervisa el proceso. Si falta el agente, puede ofrecerte instalarlo. Cuando la telemetría está activada, también puede enviar informes sanitizados de fallos de la CLI y los bridges.'],
        ['code', 'nan opencode'],
        ['h3', 'Avanzado: configuración nativa'],
        ['p', 'Usa <code>nan config &lt;agente&gt;</code> solo cuando otra herramienta o integración necesite arrancar un agente compatible mediante su ejecutable habitual. La configuración nativa escribe ajustes persistentes del proveedor; el posterior arranque directo queda fuera de la capa de compatibilidad y observabilidad de nan-harness. Este comando solo configura el agente; después, arráncalo con su ejecutable habitual:'],
        ['codes', ['nan config opencode', 'opencode']],
        ['p', 'Claude Code, Codex y fx necesitan que nan-harness siga ejecutándose porque su conexión con NaN depende de un bridge o gateway local. Por eso no se pueden preparar para uso independiente con <code>nan config</code>.'],
        ['p', 'La configuración nativa usa la API key guardada por <code>nan auth login</code>; una clave que solo está en el entorno nunca se copia a otra aplicación. Usa <code>--status</code> para revisarla y <code>--refresh</code> después de cambiar la clave guardada o el catálogo de modelos. <code>--remove</code> elimina lo que añadió nan-harness y restaura los ajustes anteriores cuando puede hacerlo de forma segura.']
      ]],
      ['options', 'OPCIONES', [
        ['h3', 'Opciones del arranque recomendado'],
        ['table', ['Opción', 'Qué hace'], [
          ['--model &lt;id&gt;', 'Qué modelo usar esta vez.'],
          ['--allow-untested', 'Ejecuta una versión del agente más nueva que las probadas con esta versión de nan-harness.'],
          ['--allow-unsupported', 'Ejecuta una versión demasiado antigua, o una cuya versión nan-harness no consigue leer.']
        ]],
        ['h3', 'Comandos avanzados de configuración nativa'],
        ['table', ['Comando', 'Qué hace'], [
          ['nan config &lt;agente&gt;', 'Escribe una configuración nativa reversible de NaN sin lanzar el agente.'],
          ['nan config &lt;agente&gt; --status', 'Comprueba si su configuración gestionada está actualizada.'],
          ['nan config &lt;agente&gt; --refresh', 'Actualiza la clave copiada, el catálogo de modelos y los valores gestionados.'],
          ['nan config &lt;agente&gt; --remove', 'Quita la configuración gestionada y restaura valores anteriores seguros.']
        ]],
        ['h3', 'El resto de comandos'],
        ['table', ['Comando', 'Qué hace'], [
          ['nan doctor', 'Revisa todo y te dice cómo está.'],
          ['nan doctor &lt;agente&gt;', 'Revisa un agente en detalle.'],
          ['nan auth login', 'Guarda tu clave de NaN.'],
          ['nan auth status', 'Te dice qué clave se está usando.'],
          ['nan auth logout', 'Borra la clave guardada y permite retirar configuraciones que contienen una copia.'],
          ['nan config --status', 'Muestra todas las configuraciones nativas gestionadas por nan-harness.'],
          ['nan config --refresh-all', 'Actualiza todas las configuraciones nativas gestionadas.'],
          ['nan config --remove-all', 'Elimina todas las configuraciones nativas gestionadas.'],
          ['nan update', 'Actualiza nan-harness a la última versión.'],
          ['nan telemetry on|off', 'Activa o desactiva la telemetría anónima.'],
          ['nan uninstall', 'Elimina nan-harness y todo lo que dejó puesto.'],
          ['nan --help', 'La lista completa, en tu terminal.']
        ]]
      ]],
      ['help', 'AYUDA Y PRIVACIDAD', [
        ['h3', 'Si algo no funciona'],
        ['p', 'Ejecuta <code>nan doctor</code>. Te dice si llega a NaN, cuántos modelos tienes disponibles, qué agentes tienes instalados y cuáles se te han quedado antiguos.'],
        ['code', 'nan doctor'],
        ['p', 'Ese informe se puede compartir sin miedo: no lleva claves, ni rutas, ni nada de lo que escribiste o respondió el modelo. La versión de un solo agente sí enseña dónde tienes el programa, así que échale un ojo antes.'],
        ['code', 'nan doctor claude'],
        ['h3', 'Avisos de versión'],
        ['p', 'Cada versión de nan-harness se prueba con versiones concretas de cada agente. Si la tuya es más nueva, te avisa y sigue. Si es más antigua de lo admitido, o nan-harness no consigue leer su versión, se para y decides tú con <code>--allow-untested</code> o <code>--allow-unsupported</code>.'],
        ['h3', 'Telemetría'],
        ['p', 'Apagada mientras no la enciendas. Si la activas, nan-harness puede enviar informes sanitizados de fallos de la CLI y los bridges, además de un evento mínimo de uso con un identificador aleatorio de instalación, su versión, el harness, la operación, el transporte, la familia del sistema operativo, la arquitectura y el entorno de destino. Nunca envía prompts, output, argumentos, rutas, modelos, credenciales, nombres de usuario ni nombres de equipo. Como en cualquier petición HTTPS, la infraestructura receptora puede observar los metadatos de red habituales.'],
        ['codes', ['nan telemetry on', 'nan telemetry off']],
        ['p', 'Nos sirve para saber qué agentes necesitan más atención. Al apagarla se detienen los envíos y se borra el identificador anónimo que los contaba.'],
        ['h3', 'Desinstalar'],
        ['p', 'Pide confirmación, deshace todas las configuraciones que dejó en tus agentes, borra tu clave guardada y se quita de en medio. Si cambiaste a mano alguna de esas configuraciones, se detiene en vez de pisarte el trabajo.'],
        ['code', 'nan uninstall']
      ]]
    ],
    guides: 'GUÍAS',
    cliReference: 'REFERENCIA CLI',
    getStartedSection: 'EMPEZAR',
    introduction: 'INTRODUCCIÓN',
    gettingStarted: 'PRIMEROS PASOS',
    examples: 'EJEMPLOS',
    agents: 'AGENTES',
    apps: 'LANDING',
    welcome: 'Bienvenido a nan-harness.',
    docsLede: 'Esta documentación explica la forma recomendada de ejecutar tus agentes de código con NaN mediante nan-harness, además de la configuración nativa avanzada para los agentes compatibles.',
    callout: '<strong>Para empezar</strong> Asegúrate de tener disponible localmente tu API key de NaN. La clave es personal y permanece en tu entorno.',
    gettingStartedText: 'Empieza con <code>nan &lt;agente&gt;</code> para que nan-harness compruebe la compatibilidad, prepare la conexión y supervise el agente. Usa la configuración nativa solo cuando necesites arrancar un agente compatible con su propio comando.',
    examplesText: 'La forma recomendada mantiene el enrutamiento del proveedor y su mantenimiento dentro de nan-harness. La configuración nativa avanzada escribe ajustes persistentes en un agente compatible.',
    whatNext: 'Qué hacer después',
    next: 'SIGUIENTE',
    copied: 'Copiado ',
    copiedStatus: 'Copiado al portapapeles.',
    telemetryCopiedStatus: 'Copiado al portapapeles. Gracias por ayudar a mejorar nan-harness.',
    copyFailed: 'No se pudo copiar. Inténtalo de nuevo.',
    faqs: [
      ['¿Qué es nan-harness?', `La forma recomendada de utilizar cualquier harness compatible con los modelos disponibles en tu ${nanLink('suscripción a la comunidad NaN')}. El comando completo del proyecto es <code>nan-harness</code>; <code>nan</code> es su alias corto.`],
      ['¿Sustituye a mi agente?', 'No. Conservas el agente original, su perfil y sus flujos. nan-harness gestiona su arranque y la conexión con NaN sin reemplazar el cliente ni escribir ajustes persistentes del proveedor.'],
      ['¿Cuándo debería usar nan config?', 'Solo cuando necesites ejecutar un agente compatible mediante su comando habitual. Para el uso diario, recomendamos <code>nan &lt;agente&gt;</code>: funciona con todos los agentes compatibles, consulta el catálogo actual, evita mantener configuraciones persistentes, comprueba la compatibilidad y puede enviar informes de error sanitizados si activas la telemetría. La configuración nativa no está disponible para Claude Code, Codex ni fx y debe actualizarse cuando cambien la clave guardada o el catálogo de modelos.'],
      ['¿Cómo se descubren los modelos?', '<code>nan &lt;agente&gt;</code> consulta el catálogo actual de NaN al arrancar. La configuración nativa avanzada escribe una copia del catálogo en el agente, así que debes ejecutar <code>nan config &lt;agente&gt; --refresh</code> cuando cambien tus modelos disponibles.'],
      ['¿Puedo pedir otro harness?', 'Sí. Si echas de menos algún harness, <a href="https://github.com/DavidLMS/nan-harness/issues/new" target="_blank" rel="noreferrer">abre un issue</a> e indícanos cuál te gustaría que incorporásemos.']
    ]
  }
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
    return `<circle class="hero-melt-dot" cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="${radius.toFixed(1)}" opacity="${opacity.toFixed(2)}"/>`;
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
    dots.push(`<circle cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="${radius.toFixed(1)}" opacity="${opacity.toFixed(2)}"/>`);
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
  const logos = logoSlots.map(([name, source, x, y], index) => `<g class="hero-melt-logo hero-melt-logo-${name}" opacity="${(1 - index * .055).toFixed(2)}"><image href="${source}" x="${x}" y="${y}" width="25" height="25" preserveAspectRatio="xMidYMid meet"/><g>${heroMeltTrail(x + 12.5, y, index)}</g></g>`).join('');

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
    <section class="hero page-width"><div class="hero-copy"><h1>${heroTitle[0]}<br><em>${heroTitle[1]}</em><br>${heroTitle[2].replace('NAN.', '<em>NAN.</em>')}</h1><p class="hero-lede">${t('heroLede')}</p><div class="hero-actions"><a class="purple-button" href="docs.html">${t('readDocs')} ${arrow()}</a><a class="text-link" href="#harness-picker">${t('seeHarnesses')} ${arrow()}</a></div>${installCommand()}</div>${heroVisual()}<div class="hero-route-row" id="harness-picker"><div class="hero-nan-lockup"><span>nan</span></div>${heroArt()}${routeCommand()}</div></section>

    <section class="section-space community-section page-width"><div class="section-number">01 <span>${t('whatIs')}</span></div><div class="section-copy"><h2>${t('oneLocal')}<br><em>${t('everyAgent')}</em></h2><p>${t('whatText')}</p><a class="text-link" href="docs.html">${t('howItWorks')} ${arrow()}</a></div></section>

    <section class="section-space feature-section page-width"><div class="section-number">02 <span>${t('workflowLabel')}</span></div><div class="terminal-wrap"><div class="terminal-head"><span>~/nan/harness</span><span>${t('workflowMode')}</span></div><div class="terminal-body"><p><span class="terminal-prompt">$</span> nan opencode</p><p class="terminal-muted">${t('managedLaunchResult')}</p><p><span class="terminal-prompt">$</span> nan config opencode</p><p class="terminal-muted">${t('nativeSetupResult')}</p><p><span class="terminal-prompt">$</span> opencode</p><p class="terminal-ok">${t('directLaunchResult')}</p><p class="terminal-cursor">_</p></div></div><div class="feature-copy"><h2>${t('workflowHeading')}<br><em>${t('workflowSubheading')}</em></h2><p>${t('workflowText')}</p></div></section>

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

const picker = document.querySelector('[data-picker]');
if (picker) {
  const control = picker.querySelector('[data-picker-control]');
  const track = picker.querySelector('[data-picker-track]');
  const items = [...picker.querySelectorAll('[data-picker-item]')];
  const semanticOptions = [...picker.querySelectorAll('[data-picker-option]')];
  const autoplayButton = picker.querySelector('[data-picker-autoplay]');
  const commandBox = document.querySelector('[data-picker-command]');
  const commandText = document.querySelector('[data-picker-command-text]');
  const commandCopy = document.querySelector('[data-picker-copy]');
  const commandStatus = document.querySelector('[data-picker-copy-status]');
  const cycleLength = harnesses.length;
  const middleStart = cycleLength * 2;
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const AUTOPLAY_INITIAL_DELAY_MS = 1000;
  const AUTOPLAY_INTERVAL_MS = 3000;
  let activePosition = middleStart;
  let autoplayTimer;
  let commandTimer;
  let commandFadeTimer;
  let commandEnterAnimation;
  let userScrollTimer;
  let scrollFrame;
  let autoplayPaused = reducedMotion.matches;
  let focusInside = false;
  let pointerInside = false;
  let pickerVisible = true;
  let touchActive = false;
  let programmaticScroll = false;
  let userScrollActive = false;
  let useInitialAutoplayDelay = true;

  picker.querySelectorAll('.picker-logo img').forEach((image) => image.addEventListener('error', () => {
    image.parentElement.classList.add('image-failed');
    image.remove();
  }));

  function hideCommand() {
    window.clearTimeout(commandFadeTimer);
    commandEnterAnimation?.cancel();
    commandEnterAnimation = null;
    commandBox.classList.add('is-hiding');
    commandFadeTimer = window.setTimeout(() => {
      if (!commandBox.classList.contains('is-hiding')) return;
      commandBox.hidden = true;
      commandBox.classList.remove('is-hiding');
      commandText.textContent = '';
      commandCopy.dataset.state = 'copy';
      commandCopy.setAttribute('aria-label', t('copyCommand'));
      commandCopy.title = t('copyCommand');
      commandStatus.textContent = '';
    }, 280);
  }

  function scheduleCommandHide() {
    window.clearTimeout(commandTimer);
    commandTimer = window.setTimeout(hideCommand, 10000);
  }

  function centerItem(item, behavior = 'smooth') {
    programmaticScroll = true;
    const scrollBehavior = reducedMotion.matches ? 'auto' : behavior;
    track.scrollTo({ top: item.offsetTop - (track.clientHeight - item.offsetHeight) / 2, behavior: scrollBehavior });
    window.setTimeout(() => { programmaticScroll = false; }, scrollBehavior === 'smooth' ? 500 : 50);
  }

  function logicalIndex(position) {
    return ((position % cycleLength) + cycleLength) % cycleLength;
  }

  function updateAutoplayControl() {
    const label = autoplayPaused ? t('resumeCarousel') : t('pauseCarousel');
    autoplayButton.dataset.state = autoplayPaused ? 'paused' : 'playing';
    autoplayButton.setAttribute('aria-label', label);
    autoplayButton.title = label;
  }

  function scheduleAutoplay() {
    window.clearTimeout(autoplayTimer);
    if (autoplayPaused || focusInside || pointerInside || touchActive || !pickerVisible || document.hidden) return;
    const delay = useInitialAutoplayDelay ? AUTOPLAY_INITIAL_DELAY_MS : AUTOPLAY_INTERVAL_MS;
    autoplayTimer = window.setTimeout(() => {
      useInitialAutoplayDelay = false;
      move(1);
      scheduleAutoplay();
    }, delay);
  }

  async function copyCommand(command) {
    commandStatus.textContent = '';
    commandCopy.dataset.state = 'copying';
    commandCopy.setAttribute('aria-label', t('copyingCommand'));
    commandCopy.title = t('copyingCommand');
    try {
      await writeClipboard(command);
      commandCopy.dataset.state = 'copied';
      commandCopy.setAttribute('aria-label', t('commandCopied'));
      commandCopy.title = t('commandCopied');
      commandStatus.textContent = t('commandCopied');
      window.setTimeout(() => {
        commandCopy.dataset.state = 'copy';
        commandCopy.setAttribute('aria-label', t('copyCommand'));
        commandCopy.title = t('copyCommand');
      }, 1600);
    } catch {
      commandCopy.dataset.state = 'copy';
      commandCopy.setAttribute('aria-label', t('copyCommand'));
      commandCopy.title = t('copyCommand');
      commandStatus.textContent = t('copyFailed');
    }
  }

  function updateCommand(position, resetTimer = false) {
    const command = harnesses[logicalIndex(position)][2];
    if (commandBox.classList.contains('is-hiding') && !resetTimer) return;
    const wasHidden = commandBox.hidden || commandBox.classList.contains('is-hiding');
    window.clearTimeout(commandFadeTimer);
    commandBox.classList.remove('is-hiding');
    commandText.textContent = command;
    commandBox.hidden = false;
    if (wasHidden) {
      commandEnterAnimation?.cancel();
      commandEnterAnimation = !reducedMotion.matches && typeof commandBox.animate === 'function'
        ? commandBox.animate([
            { opacity: 0, transform: 'translateY(12px) scale(.94)', filter: 'blur(3px)' },
            { opacity: 1, transform: 'translateY(0) scale(1)', filter: 'blur(0)' },
          ], { duration: 700, easing: 'cubic-bezier(.22, 1, .36, 1)', fill: 'forwards' })
        : null;
    }
    if (resetTimer) scheduleCommandHide();
  }

  function selectPosition(position, behavior = 'smooth', selectionOptions = {}) {
    activePosition = Math.max(0, Math.min(position, items.length - 1));
    const selectedLogicalIndex = logicalIndex(activePosition);
    items.forEach((item, itemIndex) => {
      const selected = itemIndex === activePosition;
      item.classList.toggle('is-active', selected);
    });
    semanticOptions.forEach((option, optionIndex) => option.setAttribute('aria-selected', optionIndex === selectedLogicalIndex));
    control.setAttribute('aria-activedescendant', semanticOptions[selectedLogicalIndex].id);
    if (selectionOptions.showCommand || !commandBox.hidden) updateCommand(activePosition, selectionOptions.showCommand === true);
    centerItem(items[activePosition], behavior);
  }

  function selectLogical(index, behavior = 'smooth', selectionOptions = {}) {
    selectPosition(middleStart + logicalIndex(index), behavior, selectionOptions);
  }

  function move(direction, behavior = 'smooth', selectionOptions = {}) {
    let target = activePosition + direction;
    if (target < cycleLength || target >= cycleLength * 4) {
      selectPosition(middleStart + logicalIndex(activePosition), 'auto');
      target = activePosition + direction;
    }
    selectPosition(target, behavior, selectionOptions);
  }

  function userInteracted() {
    useInitialAutoplayDelay = false;
    programmaticScroll = false;
    userScrollActive = true;
    window.clearTimeout(userScrollTimer);
    userScrollTimer = window.setTimeout(() => { userScrollActive = false; }, 1000);
    scheduleAutoplay();
  }

  function syncFromScroll() {
    if (programmaticScroll) return;
    const center = track.scrollTop + track.clientHeight / 2;
    let closestPosition = activePosition;
    let closestDistance = Infinity;
    items.forEach((item, index) => {
      const distance = Math.abs(item.offsetTop + item.offsetHeight / 2 - center);
      if (distance < closestDistance) { closestDistance = distance; closestPosition = index; }
    });
    if (closestPosition === activePosition) return;
    const target = closestPosition < cycleLength || closestPosition >= cycleLength * 4
      ? middleStart + logicalIndex(closestPosition)
      : closestPosition;
    selectPosition(target, 'auto', { showCommand: userScrollActive });
  }

  track.addEventListener('scroll', () => {
    if (scrollFrame) return;
    scrollFrame = window.requestAnimationFrame(() => { scrollFrame = null; syncFromScroll(); });
  }, { passive: true });
  track.addEventListener('wheel', userInteracted, { passive: true });
  track.addEventListener('touchstart', () => {
    touchActive = true;
    userInteracted();
    scheduleAutoplay();
  }, { passive: true });
  track.addEventListener('touchend', () => {
    touchActive = false;
    scheduleAutoplay();
  }, { passive: true });
  track.addEventListener('pointerdown', userInteracted, { passive: true });
  items.forEach((item) => item.addEventListener('click', () => {
    userInteracted();
    selectPosition(Number(item.dataset.index), 'smooth', { showCommand: true });
  }));
  autoplayButton.addEventListener('click', () => {
    autoplayPaused = !autoplayPaused;
    useInitialAutoplayDelay = false;
    updateAutoplayControl();
    scheduleAutoplay();
  });
  commandCopy.addEventListener('click', () => {
    scheduleCommandHide();
    copyCommand(commandText.textContent);
  });
  control.addEventListener('keydown', (event) => {
    if (!['ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    userInteracted();
    if (event.key === 'Home') selectLogical(0, 'smooth', { showCommand: true });
    else if (event.key === 'End') selectLogical(cycleLength - 1, 'smooth', { showCommand: true });
    else move(event.key === 'ArrowUp' || event.key === 'PageUp' ? -1 : 1, 'smooth', { showCommand: true });
  });
  picker.addEventListener('mouseenter', () => {
    pointerInside = true;
    scheduleAutoplay();
  });
  picker.addEventListener('mouseleave', () => {
    pointerInside = false;
    scheduleAutoplay();
  });
  picker.addEventListener('focusin', () => {
    focusInside = true;
    scheduleAutoplay();
  });
  picker.addEventListener('focusout', (event) => {
    if (picker.contains(event.relatedTarget)) return;
    focusInside = false;
    scheduleAutoplay();
  });
  if ('IntersectionObserver' in window) {
    const pickerObserver = new IntersectionObserver(([entry]) => {
      pickerVisible = entry.isIntersecting;
      scheduleAutoplay();
    }, { threshold: 0.2 });
    pickerObserver.observe(picker);
  }
  document.addEventListener('visibilitychange', scheduleAutoplay);
  reducedMotion.addEventListener('change', (event) => {
    if (event.matches) autoplayPaused = true;
    updateAutoplayControl();
    scheduleAutoplay();
  });
  window.requestAnimationFrame(() => selectLogical(0, 'auto'));
  const recenter = () => selectLogical(logicalIndex(activePosition), 'auto');
  window.addEventListener('load', recenter);
  let resizeTimer;
  window.addEventListener('resize', () => {
    window.clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(recenter, 160);
  });
  updateAutoplayControl();
  scheduleAutoplay();
}

document.addEventListener('click', async (event) => {
  const installTab = event.target.closest('[data-install-tab]');
  if (installTab) {
    const installBox = installTab.closest('[data-install-command]');
    const target = installTab.dataset.installTab;
    const command = installTargetCommand(target);
    const prompt = target === 'windows' ? 'PS>' : '$';
    installBox.dataset.installTarget = target;
    installBox.querySelectorAll('[data-install-tab]').forEach((tab) => {
      const selected = tab === installTab;
      tab.setAttribute('aria-selected', selected);
      tab.tabIndex = selected ? 0 : -1;
    });
    const commandPanel = installBox.querySelector('[data-install-code]');
    commandPanel.setAttribute('aria-labelledby', installTab.id);
    commandPanel.innerHTML = `<b>${prompt}</b> ${command}`;
    const copyButton = installBox.querySelector('[data-copy]');
    copyButton.dataset.copy = command;
    copyButton.dataset.state = 'copy';
    copyButton.setAttribute('aria-label', t('copyCommand'));
    copyButton.title = t('copyCommand');
    installBox.querySelector('.copy-status').textContent = '';
    return;
  }
  const localeButton = event.target.closest('[data-locale]');
  if (localeButton) {
    try {
      window.localStorage.setItem('nan-harness-locale', localeButton.dataset.locale);
    } catch {}
    window.location.reload();
    return;
  }
  const button = event.target.closest('[data-copy]');
  if (!button) return;
  const iconButton = Boolean(button.querySelector('.copy-icon'));
  const copyStatus = button.closest('.code-block')?.querySelector('.copy-status');
  const copiedStatus = button.closest('[data-telemetry-command]') ? t('telemetryCopiedStatus') : t('copiedStatus');
  if (iconButton) {
    button.dataset.state = 'copying';
    button.setAttribute('aria-label', t('copyingCommand'));
    button.title = t('copyingCommand');
  }
  try {
    await writeClipboard(button.dataset.copy);
    if (iconButton) {
      button.dataset.state = 'copied';
      button.setAttribute('aria-label', t('commandCopied'));
      button.title = t('commandCopied');
      window.setTimeout(() => {
        if (button.dataset.state !== 'copied') return;
        button.dataset.state = 'copy';
        button.setAttribute('aria-label', t('copyCommand'));
        button.title = t('copyCommand');
      }, 1600);
    } else {
      button.firstChild.textContent = `${t('copied')}`;
    }
    if (copyStatus) copyStatus.textContent = copiedStatus;
  } catch {
    if (iconButton) {
      button.dataset.state = 'copy';
      button.setAttribute('aria-label', t('copyCommand'));
      button.title = t('copyCommand');
    } else {
      button.firstChild.textContent = `${t('copy')} `;
    }
    if (copyStatus) copyStatus.textContent = t('copyFailed');
  }
});

document.addEventListener('keydown', (event) => {
  const installTab = event.target.closest('[data-install-tab]');
  if (!installTab || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const tabs = [...installTab.closest('[role="tablist"]').querySelectorAll('[data-install-tab]')];
  const currentIndex = tabs.indexOf(installTab);
  const nextIndex = event.key === 'Home'
    ? 0
    : event.key === 'End'
      ? tabs.length - 1
      : (currentIndex + (event.key === 'ArrowLeft' ? -1 : 1) + tabs.length) % tabs.length;
  tabs[nextIndex].focus();
  tabs[nextIndex].click();
});
