(() => {
  const elements = {
    shell: document.querySelector('.tv-shell'),
    runtime: document.querySelector('#runtime-context'),
    agentForm: document.querySelector('#agent-form'),
    agentInput: document.querySelector('#agent-input'),
    agentMessage: document.querySelector('#agent-message'),
    agentChat: document.querySelector('#agent-chat'),
    suggestions: document.querySelector('#suggestion-row'),
    categoryNav: document.querySelector('#category-nav'),
    catalogLeft: document.querySelector('#catalog-left'),
    catalogRight: document.querySelector('#catalog-right'),
    catalogTitleLeft: document.querySelector('#catalog-title-left'),
    catalogTitleRight: document.querySelector('#catalog-title-right'),
    catalogCountLeft: document.querySelector('#catalog-count-left'),
    catalogCountRight: document.querySelector('#catalog-count-right'),
    catalogStatus: document.querySelector('#catalog-status'),
    pagination: document.querySelector('#pagination'),
    pagePrev: document.querySelector('#page-prev'),
    pageNext: document.querySelector('#page-next'),
    pageIndicator: document.querySelector('#page-indicator'),
    episodePicker: document.querySelector('#episode-picker'),
    episodePickerTitle: document.querySelector('#episode-picker-title'),
    episodePickerMeta: document.querySelector('#episode-picker-meta'),
    episodeList: document.querySelector('#episode-list'),
    player: document.querySelector('#player'),
    video: document.querySelector('#video'),
    playerTitle: document.querySelector('#player-title'),
    playerEpisode: document.querySelector('#player-episode'),
    playerToggle: document.querySelector('#player-toggle'),
    playerRewind: document.querySelector('#player-rewind'),
    playerForward: document.querySelector('#player-forward'),
    playerSpeed: document.querySelector('#player-speed'),
    playerDanmaku: document.querySelector('#player-danmaku'),
    danmakuLayer: document.querySelector('#danmaku-layer'),
    playerPosition: document.querySelector('#player-position'),
    progressValue: document.querySelector('#progress-value'),
    playerDuration: document.querySelector('#player-duration'),
    playerFullscreen: document.querySelector('#player-fullscreen'),
    toast: document.querySelector('#toast')
  };

  const state = {
    catalog: null,
    activeCategory: 'all',
    featured: null,
    selectedDrama: null,
    selectedEpisode: null,
    pickerDrama: null,
    focusedElement: null,
    pickerReturnFocus: null,
    playerReturnFocus: null,
    controlsTimer: null,
    toastTimer: null,
    hls: null,
    hlsLoader: null,
    playbackRateIndex: 2,
    danmakuItems: [],
    danmakuCursor: 0,
    danmakuEnabled: true,
    danmakuRequestId: 0,
    busy: false,
    browsePage: 0,
    browseCategory: 'all',
    browseItems: [],
    browseHasMore: true,
    browseLoading: false,
    browseMode: 'catalog'
  };

  const CACHE_PREFIX = 'tv-agent-cache-v1:';
  const HLS_SCRIPT = 'vendor/hls.min.js';

  function readCache(key) {
    try {
      const value = localStorage.getItem(CACHE_PREFIX + key);
      return value ? JSON.parse(value) : null;
    } catch (_) {
      return null;
    }
  }

  function writeCache(key, value) {
    try {
      localStorage.setItem(CACHE_PREFIX + key, JSON.stringify(value));
    } catch (_) {
      // 存储空间不足或隐私模式禁用时继续使用内存状态。
    }
  }

  const actionHandlers = {
    'focus-search': () => {
      elements.agentInput.focus();
      elements.agentInput.select();
    },
    'voice-search': startVoiceInput,
    'select-category': element => selectCategory(element.dataset.category),
    'previous-page': () => loadCatalogPage(state.browsePage - 1),
    'next-page': () => loadCatalogPage(state.browsePage + 1),
    'open-settings': () => {
      if (window.TvAgentBridge?.openSettings) {
        window.TvAgentBridge.openSettings();
      } else if (window.aioPlugin?.openSettings) {
        window.aioPlugin.openSettings();
      } else {
        window.location.href = 'settings.html';
      }
    },
    'play-featured': () => openEpisodePicker(state.featured),
    'close-episode-picker': closeEpisodePicker,
    'close-player': closePlayer,
    'toggle-playback': togglePlayback,
    'seek-backward': () => seekBy(-15),
    'seek-forward': () => seekBy(15),
    'cycle-speed': cyclePlaybackSpeed,
    'toggle-danmaku': toggleWebDanmaku,
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
    document.documentElement.style.setProperty('--accent', drama.accent || '#d7ff64');
  }

  function renderCatalog(items) {
    elements.catalogLeft.replaceChildren();
    elements.catalogRight.replaceChildren();
    const title = catalogTitle(state.browseCategory);
    elements.catalogTitleLeft.textContent = title;
    elements.catalogTitleRight.textContent = title;
    elements.catalogCountLeft.textContent = `${items.length} 部`;
    elements.catalogCountRight.textContent = `${items.length} 部`;
    if (!items.length) {
      const message = state.browseLoading
        ? '正在从各个影视仓聚合内容……'
        : '这个分区暂时没有拉到内容，请切换分类或翻页重试。';
      [elements.catalogLeft, elements.catalogRight].forEach(container => {
        const empty = document.createElement('p');
        empty.className = 'catalog-empty';
        empty.textContent = message;
        container.append(empty);
      });
      return;
    }

    items.forEach((drama, index) => {
      const container = index % 2 === 0 ? elements.catalogLeft : elements.catalogRight;
      container.append(createDramaCard(drama, index));
    });
  }

  function catalogTitle(category) {
    return {
      all: '全部内容',
      movie: '电影',
      anime: '动漫',
      series: '电视剧',
      variety: '综艺',
      short: '短剧',
      live: '直播'
    }[category] || '全部内容';
  }

  function createDramaCard(drama, index) {
    const card = document.createElement('button');
    card.type = 'button';
    card.className = 'drama-card';
    card.dataset.focus = '';
    card.dataset.action = 'play-drama';
    card.dataset.dramaId = drama.id;
    card.setAttribute('role', 'listitem');
    card.setAttribute('aria-label', `选择${drama.title}`);
    card.innerHTML = `
      <img class="card-poster" src="${escapeHtml(drama.poster)}" alt="" loading="lazy" />
      <span class="card-index">${String(index + 1).padStart(2, '0')}</span>
      <span class="card-copy">
        <strong>${escapeHtml(drama.title)}</strong>
        <span>${escapeHtml(drama.genre || drama.subtitle)} · ${drama.episodes?.length || 0} 集</span>
      </span>
      <span class="card-meta">${escapeHtml(drama.content_type || '影视')}</span>`;
    card.addEventListener('focus', () => setFeatured(drama));
    card.addEventListener('click', () => openEpisodePicker(drama));
    return card;
  }

  function selectCategory(category) {
    const nextCategory = category || 'all';
    if (nextCategory === state.activeCategory && state.browsePage > 0) return;
    state.activeCategory = nextCategory;
    state.browseCategory = nextCategory;
    state.browseMode = 'catalog';
    elements.categoryNav.querySelectorAll('[data-category]').forEach(button => {
      button.classList.toggle('is-active', button.dataset.category === state.activeCategory);
    });
    state.browsePage = 0;
    state.browseItems = [];
    loadCatalogPage(1);
  }

  function setPagination(page, hasPrevious, hasNext) {
    elements.pagination.hidden = state.browseMode === 'search';
    elements.pageIndicator.textContent = `第 ${Math.max(1, page)} 页`;
    elements.pagePrev.disabled = !hasPrevious;
    elements.pageNext.disabled = !hasNext;
  }

  async function loadCatalogPage(pageNumber) {
    if (state.browseLoading) return;
    const targetPage = Math.max(1, Number(pageNumber) || 1);
    state.browseLoading = true;
    elements.pagePrev.disabled = true;
    elements.pageNext.disabled = true;
    const cacheKey = `browse:${state.browseCategory}:page:${targetPage}`;
    const cached = readCache(cacheKey);
    if (cached?.items?.length) {
      state.browseItems = cached.items;
      state.browsePage = Number(cached.page) || targetPage;
      state.browseHasMore = Boolean(cached.has_more);
      renderCatalog(state.browseItems);
      setFeatured(state.browseItems[0] || state.catalog?.featured);
      elements.catalogStatus.textContent = `已显示第 ${state.browsePage} 页缓存，正在后台检查`;
      setPagination(state.browsePage, state.browsePage > 1, state.browseHasMore);
    } else {
      state.browsePage = targetPage;
      state.browseItems = [];
      renderCatalog([]);
      elements.catalogStatus.textContent = `正在加载第 ${targetPage} 页……`;
      setPagination(targetPage, targetPage > 1, false);
    }
    try {
      const page = await request('GET', `/api/browse?category=${encodeURIComponent(state.browseCategory)}&page=${targetPage}&page_size=24`);
      state.browseItems = page.items || [];
      state.browsePage = Number(page.page) || targetPage;
      state.browseHasMore = Boolean(page.has_more);
      writeCache(cacheKey, {
        items: state.browseItems,
        page: state.browsePage,
        has_more: state.browseHasMore
      });
      renderCatalog(state.browseItems);
      setPagination(state.browsePage, state.browsePage > 1, state.browseHasMore);
      elements.catalogStatus.textContent = state.browseHasMore
        ? `第 ${state.browsePage} 页，每页 ${state.browseItems.length} 部`
        : `第 ${state.browsePage} 页，已是最后一页`;
      setFeatured(state.browseItems[0] || state.catalog?.featured);
      const firstCard = elements.catalogLeft.querySelector('.drama-card');
      if (firstCard) focusElement(firstCard);
    } catch (error) {
      elements.catalogStatus.textContent = `聚合失败：${error?.message || String(error)}`;
      setPagination(state.browsePage, state.browsePage > 1, state.browseHasMore);
      showToast(elements.catalogStatus.textContent);
    } finally {
      state.browseLoading = false;
    }
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

  function appendChatMessage(role, message) {
    const article = document.createElement('article');
    article.className = `chat-message is-${role}`;
    const label = document.createElement('span');
    label.className = 'chat-role';
    label.textContent = role === 'user' ? '你' : 'Agent';
    const content = document.createElement('p');
    content.textContent = String(message || '');
    article.append(label, content);
    elements.agentChat.append(article);
    elements.agentChat.scrollTop = elements.agentChat.scrollHeight;
  }

  function replaceCatalog(items, query = '') {
    state.browseItems = items;
    state.browseHasMore = false;
    state.browsePage = 1;
    state.browseMode = 'search';
    renderCatalog(items);
    elements.pagination.hidden = true;
    const normalizedQuery = String(query).trim();
    if (normalizedQuery) {
      writeCache(`search:${normalizedQuery}`, {
        items,
        page: 1,
        has_more: false
      });
    }
    elements.catalogStatus.textContent = `搜索到 ${items.length} 部`;
    if (items.length && !items.some(item => item.id === state.featured?.id)) {
      setFeatured(items[0]);
    }
  }

  async function submitAgent(message) {
    const value = String(message ?? elements.agentInput.value).trim();
    if (!value || state.busy) return;
    state.busy = true;
    appendChatMessage('user', value);
    elements.agentMessage.textContent = '正在理解你的点播……';
    elements.agentForm.classList.add('is-loading');
    try {
      const reply = await request('POST', '/api/agent', { message: value });
      elements.agentInput.value = '';
      elements.agentMessage.textContent = reply.message;
      appendChatMessage('assistant', reply.message);
      renderSuggestions(reply.suggestions || []);
      if (reply.selection) {
        setFeatured(reply.selection.drama);
        if (reply.intent === 'play') {
          openEpisodePicker(reply.selection.drama, reply.selection.episode);
        } else {
          replaceCatalog([reply.selection.drama], value);
          focusElement(`[data-drama-id="${CSS.escape(reply.selection.drama.id)}"]`);
        }
      }
    } catch (error) {
      elements.agentMessage.textContent = error?.message || String(error);
      appendChatMessage('assistant', `请求失败：${elements.agentMessage.textContent}`);
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

  function openEpisodePicker(drama, preferredEpisode) {
    if (!drama?.episodes?.length) {
      showToast('这部剧暂时没有可播放的剧集');
      return;
    }
    state.pickerDrama = drama;
    state.pickerReturnFocus = state.focusedElement || document.activeElement;
    setFeatured(drama);
    elements.episodePickerTitle.textContent = drama.title;
    elements.episodePickerMeta.textContent = `共 ${drama.episodes.length} 集，选择后开始播放`;
    elements.episodeList.replaceChildren();

    drama.episodes.forEach(episode => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'episode-option';
      button.dataset.focus = '';
      button.dataset.action = 'play-episode';
      button.dataset.episodeNumber = String(episode.number);
      button.setAttribute('role', 'listitem');
      button.setAttribute('aria-label', `播放${drama.title}第${episode.number}集`);
      button.innerHTML = `
        <strong>${String(episode.number).padStart(2, '0')}</strong>
        <span>${escapeHtml(episode.title || `第 ${episode.number} 集`)}</span>`;
      button.addEventListener('click', () => chooseEpisode(drama, episode));
      elements.episodeList.append(button);
    });

    elements.episodePicker.hidden = false;
    const preferred = preferredEpisode
      ? elements.episodeList.querySelector(`[data-episode-number="${CSS.escape(String(preferredEpisode.number))}"]`)
      : null;
    focusElement(preferred || elements.episodeList.querySelector('.episode-option'));
  }

  function chooseEpisode(drama, episode) {
    if (!drama || !episode) return;
    elements.episodePicker.hidden = true;
    state.pickerDrama = null;
    playSelection(drama, episode);
  }

  function closeEpisodePicker() {
    if (elements.episodePicker.hidden) return;
    elements.episodePicker.hidden = true;
    state.pickerDrama = null;
    focusElement(state.pickerReturnFocus);
  }

  function playSelection(drama, episode) {
    if (!drama || !episode) return;
    state.selectedDrama = drama;
    state.selectedEpisode = episode;
    state.playerReturnFocus = state.pickerReturnFocus || state.focusedElement || document.activeElement;
    setFeatured(drama);
    const episodeLabel = `第 ${episode.number} 集 · ${episode.title}`;
    if (window.TvAgentBridge?.playVideo?.(episode.video, drama.title, episodeLabel)) {
      showToast(`正在打开《${drama.title}》`);
      return;
    }
    elements.playerTitle.textContent = drama.title;
    elements.playerEpisode.textContent = episodeLabel;
    elements.video.poster = episode.poster || drama.poster;
    elements.player.hidden = false;
    elements.shell.hidden = true;
    elements.player.classList.remove('is-controls-hidden');
    elements.playerToggle.textContent = '暂停';
    resetDanmaku();
    loadVideoSource(episode.video);
    focusElement('[data-action="close-player"]');
    scheduleControlsHide();
    loadDanmaku(drama.title, episodeLabel);
  }

  function destroyHls() {
    state.hls?.destroy();
    state.hls = null;
  }

  function loadHlsRuntime() {
    if (window.Hls) return Promise.resolve(window.Hls);
    if (state.hlsLoader) return state.hlsLoader;
    state.hlsLoader = new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = HLS_SCRIPT;
      script.async = true;
      script.onload = () => window.Hls ? resolve(window.Hls) : reject(new Error('HLS 运行库加载不完整'));
      script.onerror = () => reject(new Error('HLS 运行库加载失败'));
      document.head.append(script);
    }).finally(() => {
      state.hlsLoader = null;
    });
    return state.hlsLoader;
  }

  async function loadVideoSource(url) {
    destroyHls();
    elements.video.removeAttribute('src');
    elements.video.load();
    const isHls = /\.m3u8(?:$|[?#])/i.test(url) || url.includes('.m3u8');
    if (isHls && !elements.video.canPlayType('application/vnd.apple.mpegurl')) {
      try {
        const Hls = await loadHlsRuntime();
        if (!Hls.isSupported()) {
          throw new Error('当前浏览器不支持 Media Source Extensions');
        }
        const hls = new Hls({
          enableWorker: true,
          lowLatencyMode: false,
          backBufferLength: 30
        });
        state.hls = hls;
        hls.on(Hls.Events.ERROR, (_, data) => {
          if (!data.fatal) return;
          const message = data.details || data.response?.code || 'HLS 播放错误';
          if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
            hls.startLoad();
          } else {
            hls.destroy();
            state.hls = null;
            showToast(`视频加载失败：${message}`);
          }
        });
        hls.loadSource(url);
        hls.attachMedia(elements.video);
        hls.on(Hls.Events.MANIFEST_PARSED, () => playVideo());
        return;
      } catch (error) {
        showToast(`视频加载失败：${error.message}`);
        return;
      }
    }
    elements.video.src = url;
    elements.video.load();
    playVideo();
  }

  function playVideo() {
    const playPromise = elements.video.play();
    if (playPromise?.catch) {
      playPromise.catch(error => {
        if (error?.name !== 'AbortError') showToast(`视频无法自动播放：${error.message}`);
      });
    }
  }

  function closePlayer() {
    destroyHls();
    elements.video.pause();
    elements.video.removeAttribute('src');
    elements.video.load();
    resetDanmaku();
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

  const playbackRates = [0.5, 0.75, 1, 1.25, 1.5, 2];
  const playbackRateLabels = ['0.5x', '0.75x', '1.0x', '1.25x', '1.5x', '2.0x'];

  function seekBy(seconds) {
    if (!state.selectedDrama) return;
    const duration = Number.isFinite(elements.video.duration) ? elements.video.duration : Infinity;
    elements.video.currentTime = Math.max(0, Math.min(duration, elements.video.currentTime + seconds));
    alignDanmaku(true);
    revealControls();
  }

  function cyclePlaybackSpeed() {
    state.playbackRateIndex = (state.playbackRateIndex + 1) % playbackRates.length;
    const rate = playbackRates[state.playbackRateIndex];
    elements.video.playbackRate = rate;
    elements.playerSpeed.textContent = playbackRateLabels[state.playbackRateIndex];
    revealControls();
  }

  function toggleWebDanmaku() {
    const hidden = elements.player.classList.toggle('is-danmaku-hidden');
    state.danmakuEnabled = !hidden;
    elements.playerDanmaku.textContent = hidden ? '弹幕关' : '弹幕开';
    revealControls();
  }

  async function loadDanmaku(title, episodeLabel) {
    resetDanmaku();
    if (!title) return;
    const requestId = ++state.danmakuRequestId;
    const episode = episodeLabel.split('·')[0].trim();
    try {
      const page = await request('POST', '/api/danmaku', {
        title,
        episode
      });
      if (requestId !== state.danmakuRequestId || !Array.isArray(page?.items)) return;
      state.danmakuItems = page.items
        .map(item => ({
          time: Number(item.time) || 0,
          text: String(item.text || ''),
          color: Number(item.color) || 0xffffff,
          type: Number(item.type) || 1
        }))
        .filter(item => item.text)
        .sort((left, right) => left.time - right.time);
      state.danmakuCursor = 0;
      alignDanmaku(true);
    } catch (error) {
      if (requestId === state.danmakuRequestId) showToast(`弹幕加载失败：${error.message}`);
    }
  }

  function resetDanmaku() {
    state.danmakuRequestId += 1;
    state.danmakuItems = [];
    state.danmakuCursor = 0;
    elements.danmakuLayer?.replaceChildren();
  }

  function alignDanmaku(replace) {
    const current = elements.video.currentTime || 0;
    if (replace) elements.danmakuLayer.replaceChildren();
    while (
      state.danmakuCursor < state.danmakuItems.length
      && state.danmakuItems[state.danmakuCursor].time < current - 1.2
    ) state.danmakuCursor += 1;
  }

  function renderDueDanmaku() {
    if (!state.danmakuEnabled || elements.player.hidden) return;
    const current = elements.video.currentTime || 0;
    alignDanmaku(false);
    let rendered = 0;
    while (
      state.danmakuCursor < state.danmakuItems.length
      && state.danmakuItems[state.danmakuCursor].time <= current
      && rendered < 4
    ) {
      const item = state.danmakuItems[state.danmakuCursor++];
      if (current - item.time <= 1.2) renderDanmakuItem(item);
      rendered += 1;
    }
  }

  function renderDanmakuItem(item) {
    if (!elements.danmakuLayer) return;
    const node = document.createElement('span');
    node.className = 'danmaku-item';
    node.classList.toggle('is-fixed', item.type === 4 || item.type === 5);
    if (item.type === 5) node.classList.add('is-bottom');
    node.textContent = item.text;
    node.style.color = `#${(item.color & 0xffffff).toString(16).padStart(6, '0')}`;
    node.style.top = `${6 + ((state.danmakuCursor * 7) % 46)}%`;
    node.style.animationDuration = `${Math.max(5, 8 / (elements.video.playbackRate || 1)).toFixed(1)}s`;
    elements.danmakuLayer.append(node);
    node.addEventListener('animationend', () => node.remove(), { once: true });
  }

  function toggleFullscreen() {
    if (!document.fullscreenElement) {
      document.documentElement.requestFullscreen?.().catch(error => showToast(error.message));
    } else {
      document.exitFullscreen?.().catch(() => {});
    }
  }

  document.addEventListener('fullscreenchange', () => {
    elements.playerFullscreen.textContent = document.fullscreenElement ? '退出全屏' : '全屏';
  });

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
    const root = !elements.player.hidden
      ? elements.player
      : !elements.episodePicker.hidden
        ? elements.episodePicker
        : elements.shell;
    return [...root.querySelectorAll('[data-focus]')].filter(element => (
      !element.disabled &&
      element.offsetParent !== null &&
      element.getClientRects().length > 0
    ));
  }

  function scrollableAncestor(element) {
    let parent = element.parentElement;
    while (parent && parent !== document.body) {
      const style = window.getComputedStyle(parent);
      const scrollable = style.overflowY === 'auto' || style.overflowY === 'scroll';
      if (scrollable && parent.scrollHeight > parent.clientHeight) return parent;
      parent = parent.parentElement;
    }
    return null;
  }

  function focusElement(target, options = {}) {
    const element = typeof target === 'string' ? document.querySelector(target) : target;
    if (!element) return false;
    document.querySelectorAll('[data-focus].is-focused').forEach(item => item.classList.remove('is-focused'));
    state.focusedElement = element;
    element.classList.add('is-focused');
    element.focus({ preventScroll: true });
    if (options.scroll !== false) {
      const scroller = scrollableAncestor(element);
      if (scroller) {
        const scrollerRect = scroller.getBoundingClientRect();
        const rect = element.getBoundingClientRect();
        const safeTop = scrollerRect.top + 12;
        const safeBottom = scrollerRect.bottom - 12;
        if (rect.top < safeTop) {
          scroller.scrollBy({ top: rect.top - safeTop, behavior: 'smooth' });
        } else if (rect.bottom > safeBottom) {
          scroller.scrollBy({ top: rect.bottom - safeBottom, behavior: 'smooth' });
        }
        return true;
      }
      const rect = element.getBoundingClientRect();
      const safeTop = Math.max(18, window.innerHeight * 0.055);
      const safeBottom = Math.max(28, window.innerHeight * 0.085);
      if (rect.top < safeTop) {
        window.scrollBy({ top: rect.top - safeTop, behavior: 'smooth' });
      } else if (rect.bottom > window.innerHeight - safeBottom) {
        window.scrollBy({
          top: rect.bottom - window.innerHeight + safeBottom,
          behavior: 'smooth'
        });
      }
    }
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
      const drama = [
        ...(state.catalog?.sections?.flatMap(section => section.items) || []),
        ...state.browseItems
      ]
        .find(item => item.id === element.dataset.dramaId);
      openEpisodePicker(drama);
      return;
    }
    if (action === 'play-episode') {
      element.click();
      return;
    }
    if (action === 'suggestion') {
      submitAgent(element.dataset.value);
      return;
    }
    actionHandlers[action]?.(element);
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
        if (!elements.player.hidden) {
          closePlayer();
        } else if (!elements.episodePicker.hidden) {
          closeEpisodePicker();
        }
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
    if (!elements.episodePicker.hidden) {
      closeEpisodePicker();
      return true;
    }
    return false;
  };

  document.addEventListener('keydown', handleKey);
  document.addEventListener('focusin', event => {
    const element = event.target.closest?.('[data-focus]');
    if (element && element !== state.focusedElement) focusElement(element);
  });
  document.addEventListener('click', event => {
    const target = event.target.closest?.('[data-action]');
    if (!target) return;
    const action = target.dataset.action;
    if (action === 'play-drama' || action === 'play-episode' || action === 'suggestion') return;
    actionHandlers[action]?.(target);
  });

  elements.player.addEventListener('mousemove', revealControls);
  elements.player.addEventListener('pointerdown', () => {
    if (!elements.player.hidden) revealControls();
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
    elements.video.classList.toggle(
      'is-portrait',
      elements.video.videoHeight > elements.video.videoWidth * 1.15
    );
    elements.playerDuration.textContent = formatDuration(elements.video.duration || state.selectedEpisode?.duration_seconds);
    revealControls();
  });
  elements.video.addEventListener('timeupdate', () => {
    const duration = elements.video.duration || state.selectedEpisode?.duration_seconds || 0;
    elements.playerPosition.textContent = formatDuration(elements.video.currentTime);
    elements.progressValue.style.width = duration ? `${(elements.video.currentTime / duration) * 100}%` : '0%';
    renderDueDanmaku();
  });
  elements.video.addEventListener('ended', () => {
    elements.playerToggle.textContent = '重播';
    revealControls();
  });
  elements.video.addEventListener('error', () => {
    const error = elements.video.error;
    if (!elements.player.hidden && error) {
      showToast(`视频加载失败：${error.message || `错误 ${error.code}`}`);
    }
  });

  async function load() {
    const cachedCatalog = readCache('catalog');
    const cachedContext = readCache('context');
    if (cachedCatalog?.featured) {
      state.catalog = cachedCatalog;
      setFeatured(cachedCatalog.featured);
      renderSuggestions(['我想看斗破苍穹', '来一部治愈的动物短剧', '播放咖啡奇旅']);
      elements.runtime.textContent = cachedContext?.tenant_id === 'android'
        ? 'Android TV · 缓存内容'
        : 'AIO · 已载入缓存';
      await loadCatalogPage(1);
      focusElement(elements.agentInput, { scroll: false });
    }
    try {
      const [catalog, context] = await Promise.all([
        request('GET', '/api/catalog'),
        request('GET', '/api/context')
      ]);
      state.catalog = catalog;
      state.browseItems = [];
      state.browseHasMore = true;
      const identity = context?.user_id ? `用户 ${context.user_id}` : '本机模式';
      elements.runtime.textContent = context?.tenant_id === 'android'
        ? `Android TV · ${identity}`
        : `AIO · ${context?.tenant_id || '未提供'} · ${identity}`;
      writeCache('catalog', catalog);
      writeCache('context', context);
      setFeatured(catalog.featured);
      renderSuggestions(['我想看斗破苍穹', '来一部治愈的动物短剧', '播放咖啡奇旅']);
      await loadCatalogPage(1);
      await focusElement(elements.agentInput, { scroll: false });
      elements.agentMessage.textContent = '也可以直接说题材、心情或片名';
    } catch (error) {
      elements.runtime.textContent = '连接失败';
      elements.agentMessage.textContent = error?.message || String(error);
      showToast(elements.agentMessage.textContent);
    }
  }

  load();
})();
