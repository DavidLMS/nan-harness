// Synthetic native search and Engram-shaped memory tools. No real memory is used.
import { readFileSync } from 'node:fs';
export default function (pi) {
  const parameters = {type: 'object', properties: {query: {type: 'string'}}, required: ['query']};
  for (const [name, marker] of [['web_search', 'NATIVE_SEARCH_OK'], ['mem_search', 'NATIVE_MEMORY_OK']]) {
    pi.registerTool({name, label: name, description: 'Read an isolated qualification fixture', parameters,
      async execute() {
        const state = JSON.parse(readFileSync(new URL('../memory-fixture.json', import.meta.url), 'utf8'));
        return {content: [{type: 'text', text: state[marker]}]};
      }});
  }
}
