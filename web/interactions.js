const nanHarness = window.nanHarness;

function initializeInteractions({ harnesses, t, installTargetCommand, writeClipboard }) {
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

}

initializeInteractions(nanHarness);
delete window.nanHarness;
