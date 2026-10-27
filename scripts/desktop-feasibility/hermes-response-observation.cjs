'use strict';
// Passive browser callback. Only closed counts and verdicts leave the renderer.
exports.observe = ({ prompt, marker, bindTurn }) => {
  const visible = e => { const r = e.getBoundingClientRect();
    const style = getComputedStyle(e); return r.width > 0 && r.height > 0 &&
      style.visibility !== 'hidden' && style.display !== 'none'; };
  const editors = [...document.querySelectorAll('[data-slot="composer-root"] [role="textbox"]')].filter(visible);
  const users = [...document.querySelectorAll('[data-role="user"]')].filter(visible);
  const assistants = [...document.querySelectorAll('[data-role="assistant"]')].filter(visible);
  const exactUsers = users.filter(e => e.innerText.trim() === prompt);
  const markerAssistants = assistants.filter(e => e.innerText.includes(marker));
  const boundAssistants = markerAssistants.filter(e => !bindTurn || (() => {
    const pair = e.closest('[data-slot="aui_turn-pair"]');
    if (!pair || !pair.closest('[data-slot="aui_message-group"]') || exactUsers.length !== 1) return false;
    const pairUsers = [...pair.querySelectorAll('[data-role="user"]')];
    return pairUsers.length === 1 && pairUsers[0].innerText.trim() === prompt
      && pairUsers[0].closest('[data-slot="aui_turn-pair"]') === pair;
  })());
  return {
    responseShape: {exactUserCount: Math.min(4096, exactUsers.length),
      markerAssistantCount: Math.min(4096, markerAssistants.length),
      boundMarkerAssistantCount: Math.min(4096, boundAssistants.length)},
    inputCleared: editors.length === 1 && (editors[0].value ?? editors[0].textContent).trim() === '',
    userTurnObserved: users.filter(e => e.innerText.trim() === prompt).length === 1,
    assistantTurnCount: Math.min(4096, assistants.length),
    backendFailure: (() => {
      // Frozen Hermes puts the raw error in a collapsed Details DIV, outside
      // innerText. Read only that source error leaf in the current bound turn;
      // earlier replies, tool output and the visible headline are not errors.
      let text;
      try {
        if (exactUsers.length !== 1) return 'unclassified';
        const user = exactUsers[0];
        const pair = user.closest('[data-slot="aui_turn-pair"]');
        if (!pair || !pair.closest('[data-slot="aui_message-group"]')) return 'unclassified';
        const pairUsers = [...pair.querySelectorAll('[data-role="user"]')];
        if (pairUsers.length !== 1 || pairUsers[0] !== user) return 'unclassified';
        const current = assistants.filter(e => e.getAttribute('data-slot') === 'aui_assistant-message-root'
          && e.closest('[data-slot="aui_turn-pair"]') === pair);
        if (current.length !== 1) return 'unclassified';
        const assistant = current[0];
        const contents = [...assistant.querySelectorAll('[data-slot="aui_assistant-message-content"]')]
          .filter(e => e.closest('[data-slot="aui_assistant-message-root"]') === assistant);
        if (contents.length !== 1) return 'unclassified';
        const content = contents[0];
        const alerts = [...content.querySelectorAll('[role="alert"]')]
          .filter(e => e.closest('[data-slot="aui_assistant-message-content"]') === content);
        if (alerts.length !== 1) return 'unclassified';
        const alert = alerts[0];
        const details = [...alert.querySelectorAll(':scope > div > div > details')];
        if (details.length !== 1 || details[0].closest('[role="alert"]') !== alert) return 'unclassified';
        const children = [...details[0].children];
        if (children.length !== 2 || children[0].tagName !== 'SUMMARY' || children[1].tagName !== 'DIV'
          || children[1].childElementCount !== 0) return 'unclassified';
        text = children[1].textContent;
        if (typeof text !== 'string' || !text || text.length > 65536) return 'unclassified';
      } catch { return 'unclassified'; }
      const categories = [
        ['python-import-failure', /ModuleNotFoundError|ImportError|No module named/],
        ['provider-unconfigured', /No inference provider configured|no provider configured|missing API key/i],
        ['backend-unavailable', /backend.*(?:unavailable|failed to start|not running)|gateway.*(?:not running|unavailable)/i],
        ['invalid-model', /model.*(?:not found|not configured|invalid)/i],
        ['permission-denied', /PermissionError|permission denied|EACCES/],
        ['connection-failed', /ConnectionError|connection refused|failed to connect/i],
      ].filter(([, pattern]) => pattern.test(text)).map(([category]) => category);
      return categories.length === 1 ? categories[0] : categories.length > 1 ? 'multiple' : 'unclassified';
    })(),
    responseVerified: boundAssistants.length === 1,
  };
};
