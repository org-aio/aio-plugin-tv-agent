(() => {
  const elements = {
    shell: document.querySelector('.tv-shell'),
    runtime: document.querySelector('#runtime-context'),
    heroPoster: document.querySelector('#hero-poster'),
    heroKicker: document.querySelector('#hero-kicker'),
    heroTitle: document.querySelector('#hero-title'),
    heroSynopsis: document.querySelector('#hero-synopsis'),
    heroTags: document.querySelector('#hero-tags'),
    favoriteLabel: document.querySelector('#favorite-label'),
    peekEpisode: document.querySelector('#peek-episode'),
    peekDuration: document.querySelector('#peek-duration'),
    agentForm: document.querySelector('#agent-form'),
    agentInput: document.querySelector('#agent-input'),
    agentMessage: document.querySelector('#agent-message'),
    suggestions: document.querySelector('#suggestion-row'),
    catalogTitle: document.querySelector('#catalog-title'),
    catalogCount: document.querySelector('#catalog-count'),
    catalog: document.querySelector('#catalog-row'),
    player: document.querySelector('#player'),
    video: document.querySelector('#video'),
    playerTitle: document.querySelector('#player-title'),
    playerEpisode: document.querySelector('#player-episode'),
    playerToggle: document.querySelector('#player-toggle'),
    playerPosition: document.querySelector('#player-position'),
    progressValue: document.querySelector('#progress-value'),
    playerDuration: document.querySelector('#player-duration'),
    toast: document.querySelector('#toast')
  };

  const state = {
    catalog: null,
    featured: null,
    selectedDrama: null,
    selectedEpisode: null,
    focusedElement: null,
    playerReturnFocus: null,
    controlsTimer: null,
    toastTimer: null,
    favorites: new Set(),
    busy: false
  };

  const actionHandlers = {
    'focus-search': () => {
      elements.agentInput.focus();
      elements.agentInput.select();
    },
    'voice-search': startVoiceInput,
    'play-featured': () => playSelection(state.featured, state.featured?.episodes?.[0]),
    'toggle-favorite': toggleFavorite,
    'close-player': closePlayer,
    'toggle-playback': togglePlayback,
    'toggle-fullscreen': toggleFullscreen
  };

  function request(method, path, body) {
    if (window.aioPlugin?.json) {
      return window.aioPlugin.json(method, path, body);
    }
    if (window.TvAgentBridge?.request) {
      const payload = window.TvAgentBridge.request(
        method,
        path,
        body === undefined ? '' : JSON.stringify(body)
      );
      const response = JSON.parse(payload);
      const result = response.body ? JSON.parse(response.body) : null;
      if (response.status < 200 || response.status >= 300) {
        throw new Error(result?.error || `HTTP ${response.status}`);
      }
      return Promise.resolve(result);
    }
    if (window.location.protocol === 'http:' || window.location.protocol === 'https:') {
      return fetch(path, {
        method,
        headers: body === undefined ? undefined : { 'content-type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body)
      }).then(async response => {
        const result = await response.json().catch(() => null);
        if (!response.ok) {
          throw new Error(result?.error || `HTTP ${response.status}`);
        }
        return result;
      });
    }
    return Promise.reject(new Error('请从 AIO 工作空间或电视应用打开页面'));
  }

  function escapeHtml(value) {
    const node = document.createElement('span');
    node.textContent = String(value ?? '');
    return node.innerHTML;
  }

  function formatDuration(seconds) {
    const safe = Number.isFinite(Number(seconds)) ? Math.max(0, Number(seconds)) : 0;
    const minutes = Math.floor(safe / 60);
    return `${String(minutes).padStart(2, '0')}:${String(Math.floor(safe % 60)).padStart(2, '0')}`;
  }

  function showToast(message) {
    clearTimeout(state.toastTimer);
    elements.toast.textContent = message;
    elements.toast.classList.add('is-visible');
    state.toastTimer = window.setTimeout(() => {
      elements.toast.classList.remove('is-visible');
    }, 2800);
  }

  function setFeatured(drama) {
    if (!drama) return;
    state.featured = drama;
    elements.heroPoster.src = drama.backdrop || drama.poster;
    elements.heroPoster.alt = `${drama.title} 海报`;
    elements.heroKicker.textContent = `${drama.mood} · ${drama.genre}`;
    elements.heroTitle.textContent = drama.title;
    elements.heroSynopsis.textContent = drama.synopsis;
    elements.heroTags.innerHTML = drama.tags
      .map(tag => `<span class="tag">${escapeHtml(tag)}</span>`)
      .join('');
    elements.peekEpisode.textContent = String(drama.episodes?.[0]?.number || 1).padStart(2, '0');
    elements.peekDuration.textContent = `${drama.episodes?.[0]?.duration_seconds || Math.round(drama.duration_minutes * 60)} 秒`;
    elements.favoriteLabel.textContent = state.favorites.has(drama.id) ? '已加入片单' : '加入片单';
    document.documentElement.style.setProperty('--accent', drama.accent || '#d7ff64');
  }

  function renderCatalog(items) {
    elements.catalogCount.textContent = `${items.length} 部短剧`;
    elements.catalogTitle.textContent = items.length ? '今晚值得看' : '没有找到匹配内容';
    elements.catalog.replaceChildren();
    if (!items.length) {
      const empty = document.createElement('p');
      empty.className = 'catalog-empty';
      empty.textContent = '换一个题材试试看。';
      elements.catalog.append(empty);
      return;
    }
    items.forEach((drama, index) => {
      const card = document.createElement('button');
      card.type = 'button';
      card.className = 'drama-card';
      card.dataset.focus = '';
      card.dataset.action = 'play-drama';
      card.dataset.dramaId = drama.id;
      card.setAttribute('role', 'listitem');
      card.setAttribute('aria-label', `播放${drama.title}`);
      card.innerHTML = `
        <img src="${escapeHtml(drama.poster)}" alt="" />
        <span class="card-shade" aria-hidden="true"></span>
        <span class="card-index">第 ${String(index + 1).padStart(2, '0')} 部</span>
        <span class="card-copy">
          <strong>${escapeHtml(drama.title)}</strong>
          <span>${escapeHtml(drama.subtitle)} · ${drama.episodes?.length || 0} 集</span>
        </span>`;
      card.addEventListener('focus', () => setFeatured(drama));
      card.addEventListener('click', () => playSelection(drama, drama.episodes?.[0]));
      elements.catalog.append(card);
    });
  }

  function renderSuggestions(items) {
    elements.suggestions.replaceChildren();
    items.slice(0, 4).forEach(text => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'suggestion';
      button.dataset.focus = '';
      button.dataset.action = 'suggestion';
      button.dataset.value = text;
      button.textContent = text;
      button.addEventListener('click', () => submitAgent(text));
      elements.suggestions.append(button);
    });
  }

  function replaceCatalog(items) {
    if (!state.catalog) return;
    state.catalog.items = items;
    renderCatalog(items);
    if (items.length && !items.some(item => item.id === state.featured?.id)) {
      setFeatured(items[0]);
    }
  }

  async function submitAgent(message) {
    const value = String(message ?? elements.agentInput.value).trim();
    if (!value || state.busy) return;
    state.busy = true;
    elements.agentMessage.textContent = '正在理解你的点播……';
    elements.agentForm.classList.add('is-loading');
    try {
      const reply = await request('POST', '/api/agent', { message: value });
      elements.agentInput.value = '';
      elements.agentMessage.textContent = reply.message;
      renderSuggestions(reply.suggestions || []);
      if (reply.selection) {
        setFeatured(reply.selection.drama);
        if (reply.intent === 'play') {
          playSelection(reply.selection.drama, reply.selection.episode);
        } else {
          replaceCatalog([reply.selection.drama]);
          focusElement(`[data-drama-id="${CSS.escape(reply.selection.drama.id)}"]`);
        }
      }
    } catch (error) {
      elements.agentMessage.textContent = error?.message || String(error);
      showToast(elements.agentMessage.textContent);
    } finally {
      state.busy = false;
      elements.agentForm.classList.remove('is-loading');
    }
  }

  async function startVoiceInput() {
    if (window.TvAgentBridge?.startVoiceInput?.()) {
      elements.agentMessage.textContent = '请说出想看的短剧……';
      return;
    }
    elements.agentInput.focus();
    showToast('当前设备没有可用的系统语音输入');
  }

  window.tvAgentVoiceResult = text => {
    if (!text) return;
    elements.agentInput.value = String(text);
    submitAgent(text);
  };

  function toggleFavorite() {
    const drama = state.featured;
    if (!drama) return;
    if (state.favorites.has(drama.id)) {
      state.favorites.delete(drama.id);
      elements.favoriteLabel.textContent = '加入片单';
      showToast(`已将《${drama.title}》移出片单`);
    } else {
      state.favorites.add(drama.id);
      elements.favoriteLabel.textContent = '已加入片单';
      showToast(`已将《${drama.title}》加入片单`);
    }
  }

  function playSelection(drama, episode) {
    if (!drama || !episode) return;
    state.selectedDrama = drama;
    state.selectedEpisode = episode;
    state.playerReturnFocus = state.focusedElement || document.activeElement;
    setFeatured(drama);
    elements.playerTitle.textContent = drama.title;
    elements.playerEpisode.textContent = `第 ${episode.number} 集 · ${episode.title}`;
    elements.video.poster = episode.poster || drama.poster;
    elements.video.src = episode.video;
    elements.player.hidden = false;
    elements.shell.hidden = true;
    elements.player.classList.remove('is-controls-hidden');
    elements.playerToggle.textContent = '暂停';
    elements.video.load();
    const playPromise = elements.video.play();
    if (playPromise?.catch) {
      playPromise.catch(error => {
        if (error?.name !== 'AbortError') showToast(`视频无法自动播放：${error.message}`);
      });
    }
    focusElement('[data-action="close-player"]');
    scheduleControlsHide();
  }

  function closePlayer() {
    elements.video.pause();
    elements.video.removeAttribute('src');
    elements.video.load();
    elements.player.hidden = true;
    elements.shell.hidden = false;
    document.exitFullscreen?.().catch(() => {});
    state.selectedDrama = null;
    state.selectedEpisode = null;
    focusElement(state.playerReturnFocus);
  }

  function togglePlayback() {
    if (!state.selectedDrama) return;
    if (elements.video.paused) {
      elements.video.play().catch(error => showToast(error.message));
    } else {
      elements.video.pause();
    }
    revealControls();
  }

  function toggleFullscreen() {
    if (!document.fullscreenElement) {
      document.documentElement.requestFullscreen?.().catch(error => showToast(error.message));
    } else {
      document.exitFullscreen?.().catch(() => {});
    }
  }

  function scheduleControlsHide() {
    clearTimeout(state.controlsTimer);
    state.controlsTimer = window.setTimeout(() => {
      if (!elements.player.hidden && !elements.video.paused) {
        elements.player.classList.add('is-controls-hidden');
      }
    }, 4200);
  }

  function revealControls() {
    elements.player.classList.remove('is-controls-hidden');
    scheduleControlsHide();
  }

  function visibleFocusableElements() {
    return [...document.querySelectorAll('[data-focus]')].filter(element => (
      !element.disabled &&
      element.offsetParent !== null &&
      element.getClientRects().length > 0
    ));
  }

  function focusElement(target) {
    const element = typeof target === 'string' ? document.querySelector(target) : target;
    if (!element) return false;
    document.querySelectorAll('[data-focus].is-focused').forEach(item => item.classList.remove('is-focused'));
    state.focusedElement = element;
    element.classList.add('is-focused');
    element.focus({ preventScroll: true });
    element.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'smooth' });
    return true;
  }

  function moveFocus(direction) {
    const focusables = visibleFocusableElements();
    if (!focusables.length) return;
    const current = state.focusedElement && focusables.includes(state.focusedElement)
      ? state.focusedElement
      : document.activeElement && focusables.includes(document.activeElement)
        ? document.activeElement
        : focusables[0];
    const currentRect = current.getBoundingClientRect();
    const center = { x: currentRect.left + currentRect.width / 2, y: currentRect.top + currentRect.height / 2 };
    let best = null;
    for (const candidate of focusables) {
      if (candidate === current) continue;
      const rect = candidate.getBoundingClientRect();
      const point = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
      const dx = point.x - center.x;
      const dy = point.y - center.y;
      const inDirection = {
        ArrowLeft: dx < -8,
        ArrowRight: dx > 8,
        ArrowUp: dy < -8,
        ArrowDown: dy > 8
      }[direction];
      if (!inDirection) continue;
      const primary = direction === 'ArrowLeft' || direction === 'ArrowRight' ? Math.abs(dx) : Math.abs(dy);
      const secondary = direction === 'ArrowLeft' || direction === 'ArrowRight' ? Math.abs(dy) : Math.abs(dx);
      const overlap = direction === 'ArrowLeft' || direction === 'ArrowRight'
        ? Math.max(0, Math.min(currentRect.bottom, rect.bottom) - Math.max(currentRect.top, rect.top))
        : Math.max(0, Math.min(currentRect.right, rect.right) - Math.max(currentRect.left, rect.left));
      const score = primary + secondary * 2.2 - overlap * 0.35;
      if (!best || score < best.score) best = { element: candidate, score };
    }
    focusElement(best?.element || current);
  }

  function activateFocused() {
    const element = state.focusedElement || document.activeElement;
    if (!(element instanceof HTMLElement)) return;
    if (element === elements.agentInput) {
      elements.agentForm.requestSubmit();
      return;
    }
    const action = element.dataset.action;
    if (action === 'play-drama') {
      const drama = state.catalog?.items.find(item => item.id === element.dataset.dramaId);
      playSelection(drama, drama?.episodes?.[0]);
      return;
    }
    if (action === 'suggestion') {
      submitAgent(element.dataset.value);
      return;
    }
    actionHandlers[action]?.();
  }

  function handleKey(event) {
    if (!elements.player.hidden) revealControls();
    const keyActions = {
      ArrowLeft: () => moveFocus('ArrowLeft'),
      ArrowRight: () => moveFocus('ArrowRight'),
      ArrowUp: () => moveFocus('ArrowUp'),
      ArrowDown: () => moveFocus('ArrowDown'),
      Enter: activateFocused,
      Escape: () => {
        if (!elements.player.hidden) closePlayer();
      },
      ' ': togglePlayback
    };
    const action = keyActions[event.key];
    if (!action) return;
    if (
      document.activeElement === elements.agentInput
      && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
    ) return;
    event.preventDefault();
    action();
  }

  window.tvAgentKey = key => handleKey({ key, preventDefault() {} });
  window.tvAgentBack = () => {
    if (!elements.player.hidden) {
      closePlayer();
      return true;
    }
    return false;
  };

  document.addEventListener('keydown', handleKey);
  document.addEventListener('focusin', event => {
    const element = event.target.closest?.('[data-focus]');
    if (element) focusElement(element);
  });
  document.addEventListener('click', event => {
    const target = event.target.closest?.('[data-action]');
    if (!target) return;
    const action = target.dataset.action;
    if (action === 'play-drama' || action === 'suggestion') return;
    actionHandlers[action]?.();
  });

  elements.agentForm.addEventListener('submit', event => {
    event.preventDefault();
    submitAgent();
  });

  elements.video.addEventListener('play', () => {
    elements.playerToggle.textContent = '暂停';
    scheduleControlsHide();
  });
  elements.video.addEventListener('pause', () => {
    elements.playerToggle.textContent = '播放';
    revealControls();
  });
  elements.video.addEventListener('loadedmetadata', () => {
    elements.playerDuration.textContent = formatDuration(elements.video.duration || state.selectedEpisode?.duration_seconds);
    revealControls();
  });
  elements.video.addEventListener('timeupdate', () => {
    const duration = elements.video.duration || state.selectedEpisode?.duration_seconds || 0;
    elements.playerPosition.textContent = formatDuration(elements.video.currentTime);
    elements.progressValue.style.width = duration ? `${(elements.video.currentTime / duration) * 100}%` : '0%';
  });
  elements.video.addEventListener('ended', () => {
    elements.playerToggle.textContent = '重播';
    revealControls();
  });

  async function load() {
    try {
      const [catalog, context] = await Promise.all([
        request('GET', '/api/catalog'),
        request('GET', '/api/context')
      ]);
      state.catalog = catalog;
      const identity = context?.user_id ? `用户 ${context.user_id}` : '本机模式';
      elements.runtime.textContent = context?.tenant_id === 'android'
        ? `Android TV · ${identity}`
        : `AIO · ${context?.tenant_id || '未提供'} · ${identity}`;
      setFeatured(catalog.featured);
      renderCatalog(catalog.items);
      renderSuggestions(['来一部治愈的动物短剧', '播放咖啡奇旅', '轻松搞笑的短剧']);
      await focusElement(elements.agentInput);
      elements.agentMessage.textContent = '也可以直接说题材、心情或片名';
    } catch (error) {
      elements.runtime.textContent = '连接失败';
      elements.agentMessage.textContent = error?.message || String(error);
      showToast(elements.agentMessage.textContent);
    }
  }

  load();
})();
