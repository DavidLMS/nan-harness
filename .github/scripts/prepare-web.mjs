import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function prepareWeb(destination) {
  fs.cpSync('web', destination, { recursive: true });
  // Cached HTML may load app.js without the newly extracted locale scripts.
  const source = ['content-en.js', 'content-es.js', 'app.js']
    .map((name) => fs.readFileSync(path.join('web', name), 'utf8'))
    .join('\n;\n');
  fs.writeFileSync(path.join(destination, 'app.js'), source);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const destination = process.argv[2];
  if (!destination) throw new Error('Usage: node prepare-web.mjs <staging-directory>');
  prepareWeb(destination);
}
