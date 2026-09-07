import fs from 'node:fs';
import vm from 'node:vm';

export function siteScripts() {
  return ['web/content-en.js', 'web/content-es.js', 'web/app.js']
    .map((file) => [file, fs.readFileSync(file, 'utf8')]);
}

// Run the trusted site renderer for static generation and contract checks.
export function renderWeb(page, locale = 'en', scripts = siteScripts(), runtime = {}) {
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
  if (runtime.modelContext) document.modelContext = runtime.modelContext;
  const window = {
    localStorage: { getItem: (key) => key === 'nan-harness-locale' ? locale : null },
    ...(runtime.window ?? {}),
  };
  const navigator = {
    language: 'en',
    languages: ['en'],
    userAgent: 'test',
  };

  const context = vm.createContext({
    document,
    navigator,
    window,
    ...(runtime.AbortController ? { AbortController: runtime.AbortController } : {}),
  });
  for (const [path, source] of scripts) {
    vm.runInContext(source, context, { filename: path });
  }
  return {
    html: app.innerHTML,
    bodyClass: document.body.className,
    title: document.title,
    description: meta.content,
    copy: vm.runInContext(`translations[${JSON.stringify(locale)}]`, context),
  };
}
