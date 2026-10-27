'use strict';

// Frozen 0.17.6 ChooseLaterLink only clears first-run UI and persists its skip.
// Finish that ordinary UI action before a failed-turn identity is retained;
// the later Retry path still rejects every replacement of its retained root.
async function prepare({ page, owned, deadline, sample, stable, pause,
  now = Date.now }) {
  let dispatched = false;
  let originalDocument;
  const limit = Math.min(deadline, now() + 5000);
  const url = page.url();
  const bounded = async operation => {
    if (now() >= limit) throw Error('deadline');
    let timer;
    try {
      const result = await Promise.race([operation(), new Promise((_, reject) => {
        timer = setTimeout(() => reject(Error('deadline')), Math.max(1, limit - now()));
      })]);
      if (now() >= limit) throw Error('deadline');
      return result;
    } finally { clearTimeout(timer); }
  };
  const sameDocument = held => held === document;
  const current = async () => now() < limit && owned() && page.url() === url
    && await bounded(() => page.evaluate(sameDocument, originalDocument)) && now() < limit;
  try {
    if (now() >= limit || !owned()) throw Error('ownership');
    originalDocument = await bounded(() => page.evaluateHandle(() => document));
    const cover = page.locator('[data-glass-opaque][class~="z-(--z-onboarding)"]');
    if (!await current()) throw Error('ownership');
    const covers = await bounded(() => cover.count());
    if (covers === 0) {
      if (!await current()) throw Error('ownership');
      return { ready: true, stage: 'absent' };
    }
    if (covers !== 1 || !await bounded(() => cover.isVisible())) throw Error('cover');
    const choice = cover.getByRole('button', { name: "I'll choose a provider later", exact: true });
    while (await bounded(() => choice.count()) === 0) {
      if (!await current()) throw Error('ownership');
      await pause(Math.min(20, Math.max(0, limit - now())));
    }
    if (await bounded(() => choice.count()) !== 1 || !await bounded(() => choice.isEnabled())) throw Error('choice');
    const button = await bounded(() => choice.elementHandle());
    const belongs = element => {
      const root = element.closest('[data-glass-opaque]');
      return element.ownerDocument === document && element.isConnected
        && root?.ownerDocument === document && root.isConnected
        && root.classList.contains('z-(--z-onboarding)');
    };
    if (!button || !await bounded(() => button.evaluate(belongs))) throw Error('choice');
    const first = await bounded(() => button.evaluate(sample));
    await pause(Math.min(100, Math.max(0, limit - now())));
    const second = await bounded(() => button.evaluate(sample));
    const position = stable(first, second);
    if (!position || !await current() || await bounded(() => cover.count()) !== 1
        || await bounded(() => choice.count()) !== 1 || !await bounded(() => choice.isEnabled())
        || !await bounded(() => choice.evaluate((element, held) => element === held, button))
        || !await bounded(() => button.evaluate(belongs))) throw Error('choice');
    const final = await bounded(() => button.evaluate(sample));
    if (!stable(second, final) || !await current()) throw Error('choice');
    await bounded(() => {
      const remaining = limit - now();
      if (remaining <= 0) throw Error('deadline');
      dispatched = true;
      return button.click({ position, timeout: remaining });
    });
    while (await bounded(() => cover.count()) !== 0) {
      if (!await current()) throw Error('ownership');
      await pause(Math.min(20, Math.max(0, limit - now())));
    }
    // The ordinary dismissal can replace the whole view. No failed-turn lease
    // exists yet: require the current owned composer to settle before typing.
    const composer = page.locator('[data-slot="composer-root"] [role="textbox"][contenteditable="true"]:visible:not([aria-disabled="true"])');
    while (await current()) {
      const count = await bounded(() => composer.count());
      if (count > 1) throw Error('composer');
      if (count === 1) {
        const held = await bounded(() => composer.elementHandle());
        await pause(Math.min(100, Math.max(0, limit - now())));
        if (held && await current() && await bounded(() => cover.count()) === 0
            && await bounded(() => composer.count()) === 1
            && await bounded(() => composer.evaluate((element, original) => element === original, held)))
          return { ready: true, stage: 'dismissed' };
      } else await pause(Math.min(20, Math.max(0, limit - now())));
    }
  } catch { /* No uncertain dismissal is repeated or authorizes later input. */ }
  return { ready: false, stage: dispatched ? 'uncertain' : 'blocked' };
}

module.exports = { prepare };
