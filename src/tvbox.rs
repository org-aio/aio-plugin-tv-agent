use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::task::JoinSet;

use crate::{
    catalog::{Drama, Episode},
    config::{BrokerConfig, same_http_endpoint},
};

const MAX_RESPONSE_BYTES: usize = 512_000;
const MAX_CONFIG_RESPONSE_BYTES: usize = 4_000_000;
const MAX_LIVE_RESPONSE_BYTES: usize = 2_000_000;
const MAX_CONFIG_INDEX_ENTRIES: usize = 12;
const MAX_SEARCH_SOURCES: usize = 24;
const MAX_BROWSE_SOURCES: usize = 12;
const MAX_BROWSE_PAGE_SIZE: usize = 40;
const MAX_LIVE_PLAYLISTS: usize = 12;
const SOURCE_SEARCH_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_BROWSE_TIMEOUT: Duration = Duration::from_secs(8);
const LIVE_PLAYLIST_TIMEOUT: Duration = Duration::from_secs(8);
const TOTAL_SEARCH_TIMEOUT: Duration = Duration::from_secs(12);
const TOTAL_BROWSE_TIMEOUT: Duration = Duration::from_secs(14);
const TOTAL_LIVE_TIMEOUT: Duration = Duration::from_secs(12);
const BROWSE_CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const SOURCE_CACHE_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
struct CacheEntry<T> {
    value: T,
    created_at: Instant,
}

#[derive(Default)]
struct CacheStore {
    sources: HashMap<String, CacheEntry<Vec<Source>>>,
    browse: HashMap<String, CacheEntry<BrowsePage>>,
}

#[derive(Clone)]
pub(crate) struct TvBoxClient {
    client: Client,
    broker: Option<BrokerConfig>,
    cache: Arc<Mutex<CacheStore>>,
}

#[derive(Clone, Debug)]
struct Source {
    name: String,
    api: String,
}

#[derive(Debug, Deserialize)]
struct TvBoxConfig {
    #[serde(default)]
    sites: Vec<TvBoxSite>,
    #[serde(default)]
    lives: Vec<TvBoxLive>,
    #[serde(default)]
    urls: Option<Vec<TvBoxIndexEntry>>,
}

#[derive(Debug, Deserialize)]
struct TvBoxLive {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    r#type: i64,
}

#[derive(Debug, Deserialize)]
struct TvBoxIndexEntry {
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize)]
struct TvBoxSite {
    #[serde(default)]
    key: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    api: String,
    #[serde(default)]
    r#type: i64,
    #[serde(default = "default_searchable")]
    searchable: i64,
}

#[derive(Debug, Deserialize)]
struct VodList {
    #[serde(default)]
    list: Vec<Vod>,
    #[serde(default)]
    class: Vec<VodClass>,
    #[serde(default, deserialize_with = "string_or_number")]
    total: String,
}

#[derive(Clone, Debug, Deserialize)]
struct VodClass {
    #[serde(default, deserialize_with = "string_or_number")]
    type_id: String,
    #[serde(default, deserialize_with = "string_or_number")]
    type_pid: String,
    #[serde(default)]
    type_name: String,
}

#[derive(Clone, Debug)]
struct BrowseSourcePage {
    items: Vec<Drama>,
    total: u32,
    has_more: bool,
}

#[derive(Clone, Debug)]
struct LivePlaylist {
    name: String,
    url: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct BrowsePage {
    pub items: Vec<Drama>,
    pub page: u32,
    pub next_page: u32,
    pub has_more: bool,
    pub total: u32,
    pub category: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Vod {
    #[serde(default, deserialize_with = "string_or_number")]
    vod_id: String,
    #[serde(default)]
    vod_name: String,
    #[serde(default)]
    vod_sub: String,
    #[serde(default)]
    vod_pic: String,
    #[serde(default)]
    vod_remarks: String,
    #[serde(default)]
    vod_class: String,
    #[serde(default)]
    vod_content: String,
    #[serde(default)]
    vod_blurb: String,
    #[serde(default)]
    vod_play_from: String,
    #[serde(default)]
    vod_play_url: String,
    #[serde(default)]
    type_name: String,
}

impl TvBoxClient {
    pub(crate) fn new(broker: Option<BrokerConfig>) -> Result<Self> {
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20));
        if let Some(broker) = &broker {
            builder = builder.unix_socket(broker.socket.clone());
        }
        let client = builder.build()?;
        Ok(Self {
            client,
            broker,
            cache: Arc::new(Mutex::new(CacheStore::default())),
        })
    }

    pub(crate) async fn search(
        &self,
        query: &str,
        config_urls: &[String],
    ) -> Result<Option<Drama>> {
        let query = query.trim();
        ensure!(!query.is_empty(), "搜索关键词不能为空");
        let sources = self.sources(config_urls, query).await?;
        let mut last_error = None;
        let mut searches = JoinSet::new();
        let source_count = sources.len().min(MAX_SEARCH_SOURCES);

        for (index, source) in sources.into_iter().take(MAX_SEARCH_SOURCES).enumerate() {
            let client = self.clone();
            let query = query.to_owned();
            searches.spawn(async move {
                let name = source.name.clone();
                let result = tokio::time::timeout(
                    SOURCE_SEARCH_TIMEOUT,
                    client.search_source(&source, &query),
                )
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("影视源 {name} 搜索超时")));
                (index, result)
            });
        }
        let mut results: Vec<Option<Result<Option<Drama>>>> =
            (0..source_count).map(|_| None).collect();
        let deadline = tokio::time::Instant::now() + TOTAL_SEARCH_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                searches.abort_all();
                break;
            }
            match tokio::time::timeout(remaining, searches.join_next()).await {
                Ok(Some(Ok((index, result)))) => results[index] = Some(result),
                Ok(Some(Err(error))) => last_error = Some(anyhow::Error::new(error)),
                Ok(None) => break,
                Err(_) => {
                    searches.abort_all();
                    last_error = Some(anyhow::anyhow!("影视源搜索总超时"));
                    break;
                }
            }
        }
        for result in results.into_iter().flatten() {
            match result {
                Ok(Some(drama)) => return Ok(Some(drama)),
                Ok(None) => {}
                Err(error) => last_error = Some(error),
            }
        }
        if let Some(error) = last_error {
            return Err(error.context("所有影视源均未返回可播放结果"));
        }
        Ok(None)
    }

    pub(crate) async fn browse(
        &self,
        config_urls: &[String],
        category: &str,
        page: u32,
        page_size: usize,
    ) -> Result<BrowsePage> {
        let category = category.trim();
        let page = page.max(1);
        let page_size = page_size.clamp(1, MAX_BROWSE_PAGE_SIZE);
        let cache_key = format!(
            "{}\n---\n{category}:{page}:{page_size}",
            config_urls.join("\n")
        );
        if let Some(cached) = self.cached_browse(&cache_key, BROWSE_CACHE_TTL) {
            return Ok(cached);
        }
        let stale = self.cached_browse(&cache_key, Duration::MAX);
        if let Some(stale_value) = stale {
            let client = self.clone();
            let config_urls = config_urls.to_vec();
            let category = category.to_owned();
            let key = cache_key.clone();
            tokio::spawn(async move {
                if let Ok(page) = client
                    .browse_uncached(&config_urls, &category, page, page_size)
                    .await
                {
                    client.store_browse(key, page);
                }
            });
            return Ok(stale_value);
        }
        let result = self
            .browse_uncached(config_urls, category, page, page_size)
            .await;
        match result {
            Ok(page) => {
                self.store_browse(cache_key, page.clone());
                Ok(page)
            }
            Err(error) => Err(error),
        }
    }

    async fn browse_uncached(
        &self,
        config_urls: &[String],
        category: &str,
        page: u32,
        page_size: usize,
    ) -> Result<BrowsePage> {
        if category == "live" {
            return self.live_page(config_urls, page, page_size).await;
        }

        let sources = self.sources(config_urls, "").await?;
        let mut requests = JoinSet::new();
        let source_count = sources.len().min(MAX_BROWSE_SOURCES);
        for (index, source) in sources.into_iter().take(MAX_BROWSE_SOURCES).enumerate() {
            let client = self.clone();
            let category = category.to_owned();
            requests.spawn(async move {
                let result = tokio::time::timeout(
                    SOURCE_BROWSE_TIMEOUT,
                    client.browse_source(&source, &category, page),
                )
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("影视源浏览超时")));
                (index, result)
            });
        }

        let mut results: Vec<Option<Result<BrowseSourcePage>>> =
            (0..source_count).map(|_| None).collect();
        let deadline = tokio::time::Instant::now() + TOTAL_BROWSE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                requests.abort_all();
                break;
            }
            match tokio::time::timeout(remaining, requests.join_next()).await {
                Ok(Some(Ok((index, result)))) => results[index] = Some(result),
                Ok(Some(Err(_))) | Ok(None) | Err(_) => {
                    requests.abort_all();
                    break;
                }
            }
        }

        let mut items = Vec::new();
        let mut seen = HashSet::new();
        let mut source_has_more = false;
        let mut total = 0u32;
        for result in results.into_iter().flatten() {
            let Ok(page_result) = result else {
                continue;
            };
            total = total.saturating_add(page_result.total);
            source_has_more |= page_result.has_more;
            for drama in page_result.items {
                if !category.is_empty() && category != "all" && drama.content_type != category {
                    continue;
                }
                let key = format!("{}|{}", drama.title, drama.content_type);
                if seen.insert(key) {
                    items.push(drama);
                }
            }
        }
        items.truncate(page_size);
        let has_more = source_has_more && !items.is_empty();
        Ok(BrowsePage {
            items,
            page,
            next_page: if has_more { page + 1 } else { 0 },
            has_more,
            total,
            category: category.to_owned(),
        })
    }

    async fn sources(&self, config_urls: &[String], query: &str) -> Result<Vec<Source>> {
        let cache_key = config_urls.join("\n");
        if let Some(cached) = self.cached_sources(&cache_key, SOURCE_CACHE_TTL) {
            return Ok(rerank_sources(cached, query));
        }
        let stale = self.cached_sources(&cache_key, Duration::MAX);
        let result = self.sources_uncached(config_urls).await;
        match result {
            Ok(sources) => {
                self.store_sources(cache_key, sources.clone());
                Ok(rerank_sources(sources, query))
            }
            Err(error) => {
                if let Some(sources) = stale {
                    return Ok(rerank_sources(sources, query));
                }
                Err(error)
            }
        }
    }

    async fn sources_uncached(&self, config_urls: &[String]) -> Result<Vec<Source>> {
        let mut sources = fallback_sources();
        let mut seen = sources
            .iter()
            .map(|source| source.api.clone())
            .collect::<HashSet<_>>();
        let mut last_error = None;
        let mut tasks = Vec::with_capacity(config_urls.len());
        for config_url in config_urls {
            let client = self.clone();
            let config_url = config_url.clone();
            tasks.push(tokio::spawn(async move {
                client.sources_from_url(&config_url).await
            }));
        }
        for task in tasks {
            match task.await {
                Ok(Ok(mut configured)) => {
                    for source in configured.drain(..) {
                        if seen.insert(source.api.clone()) {
                            sources.push(source);
                        }
                    }
                }
                Ok(Err(error)) => last_error = Some(error),
                Err(error) => last_error = Some(anyhow::Error::new(error)),
            }
        }
        if sources.is_empty()
            && let Some(error) = last_error
        {
            return Err(error.context("没有可用的 TVBox 配置上游"));
        }
        Ok(sources)
    }

    fn cached_sources(&self, key: &str, ttl: Duration) -> Option<Vec<Source>> {
        let cache = self.cache.lock().ok()?;
        let entry = cache.sources.get(key)?;
        (entry.created_at.elapsed() <= ttl).then(|| entry.value.clone())
    }

    fn store_sources(&self, key: String, sources: Vec<Source>) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.sources.insert(
                key,
                CacheEntry {
                    value: sources,
                    created_at: Instant::now(),
                },
            );
        }
    }

    fn cached_browse(&self, key: &str, ttl: Duration) -> Option<BrowsePage> {
        let cache = self.cache.lock().ok()?;
        let entry = cache.browse.get(key)?;
        (entry.created_at.elapsed() <= ttl).then(|| entry.value.clone())
    }

    fn store_browse(&self, key: String, page: BrowsePage) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.browse.insert(
                key,
                CacheEntry {
                    value: page,
                    created_at: Instant::now(),
                },
            );
        }
    }

    async fn sources_from_url(&self, config_url: &str) -> Result<Vec<Source>> {
        let config: TvBoxConfig = self
            .get_config_json(config_url)
            .await
            .with_context(|| format!("读取 TVBox 配置失败: {config_url}"))?;
        if let Some(urls) = config.urls {
            let mut sources = Vec::new();
            for entry in urls.into_iter().take(MAX_CONFIG_INDEX_ENTRIES) {
                let Some(url) = normalized_https_url(&entry.url) else {
                    continue;
                };
                let nested: TvBoxConfig = match self.get_config_json(&url).await {
                    Ok(config) => config,
                    Err(_) => continue,
                };
                sources.extend(self.sources_from_config(nested));
            }
            return Ok(sources);
        }
        Ok(self.sources_from_config(config))
    }

    fn sources_from_config(&self, config: TvBoxConfig) -> Vec<Source> {
        let mut sources = Vec::new();
        for site in config.sites {
            if site.r#type != 1 || site.searchable == 0 {
                continue;
            }
            let Some(api) = normalized_api(&site.api) else {
                continue;
            };
            if !api.starts_with("https://") || !self.is_authorized(&api) {
                continue;
            }
            let name = if site.name.trim().is_empty() {
                site.key
            } else {
                site.name
            };
            sources.push(Source { name, api });
        }
        sources
    }

    async fn search_source(&self, source: &Source, query: &str) -> Result<Option<Drama>> {
        let list_url = with_query(&source.api, &[("ac", "list"), ("wd", query)])?;
        let list: VodList = self.get_json(list_url.as_str()).await?;
        let Some(candidate) = best_candidate(&list.list, query) else {
            return Ok(None);
        };
        ensure!(!candidate.vod_id.trim().is_empty(), "影视源返回了空 ID");

        let detail_url = with_query(
            &source.api,
            &[("ac", "detail"), ("ids", candidate.vod_id.trim())],
        )?;
        let detail: VodList = self.get_json(detail_url.as_str()).await?;
        let Some(vod) = detail.list.into_iter().next() else {
            return Ok(None);
        };
        let Some(drama) = to_drama(&vod) else {
            return Ok(None);
        };
        Ok(Some(drama))
    }

    async fn browse_source(
        &self,
        source: &Source,
        category: &str,
        page: u32,
    ) -> Result<BrowseSourcePage> {
        let page_value = page.to_string();
        let page_size = MAX_BROWSE_PAGE_SIZE.to_string();
        let class_id = self.browse_class_id(source, category).await?;
        let mut query = vec![
            ("ac", "detail"),
            ("pg", page_value.as_str()),
            ("pagesize", page_size.as_str()),
            ("limit", page_size.as_str()),
        ];
        if let Some(class_id) = class_id.as_deref() {
            query.push(("t", class_id));
        }
        let list_url = with_query(&source.api, &query)?;
        let list: VodList = self.get_json(list_url.as_str()).await?;
        let mut items = list.list.iter().filter_map(to_drama).collect::<Vec<_>>();
        if category != "all" && !category.is_empty() {
            items.retain(|drama| drama.content_type == category);
        }
        Ok(BrowseSourcePage {
            has_more: items.len() >= MAX_BROWSE_PAGE_SIZE,
            items,
            total: parse_count(&list.total).unwrap_or_default(),
        })
    }

    async fn browse_class_id(&self, source: &Source, category: &str) -> Result<Option<String>> {
        if category.is_empty() || category == "all" {
            return Ok(None);
        }
        let list_url = with_query(&source.api, &[("ac", "list")])?;
        let list: VodList = self.get_json(list_url.as_str()).await?;
        Ok(best_class(&list.class, category).map(|class| class.type_id.clone()))
    }

    async fn live_page(
        &self,
        config_urls: &[String],
        page: u32,
        page_size: usize,
    ) -> Result<BrowsePage> {
        let playlists = self.live_playlists(config_urls).await?;
        let mut tasks = JoinSet::new();
        for (index, playlist) in playlists.into_iter().take(MAX_LIVE_PLAYLISTS).enumerate() {
            let client = self.clone();
            tasks.spawn(async move {
                let result = tokio::time::timeout(
                    LIVE_PLAYLIST_TIMEOUT,
                    client.load_live_playlist(&playlist),
                )
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("直播列表读取超时")));
                (index, result)
            });
        }

        let mut items = Vec::new();
        let mut seen = HashSet::new();
        let deadline = tokio::time::Instant::now() + TOTAL_LIVE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                tasks.abort_all();
                break;
            }
            match tokio::time::timeout(remaining, tasks.join_next()).await {
                Ok(Some(Ok((_, Ok(channels))))) => {
                    for channel in channels {
                        if seen.insert(format!("{}|{}", channel.title, channel.episodes[0].video)) {
                            items.push(channel);
                        }
                    }
                }
                Ok(Some(Ok((_, Err(_))))) | Ok(Some(Err(_))) => {}
                Ok(None) | Err(_) => {
                    tasks.abort_all();
                    break;
                }
            }
        }

        let start = (page.saturating_sub(1) as usize).saturating_mul(page_size);
        let has_more = items.len() > start.saturating_add(page_size);
        let items = items
            .into_iter()
            .skip(start)
            .take(page_size)
            .collect::<Vec<_>>();
        let total = start.saturating_add(items.len()) as u32;
        Ok(BrowsePage {
            items,
            page,
            next_page: if has_more { page + 1 } else { 0 },
            has_more,
            total,
            category: "live".into(),
        })
    }

    async fn live_playlists(&self, config_urls: &[String]) -> Result<Vec<LivePlaylist>> {
        let mut playlists = fallback_live_playlists()
            .into_iter()
            .filter(|playlist| {
                self.broker
                    .as_ref()
                    .is_none_or(|_| self.is_authorized(&playlist.url))
            })
            .collect::<Vec<_>>();
        let mut seen = playlists
            .iter()
            .map(|playlist| playlist.url.clone())
            .collect::<HashSet<_>>();
        let mut tasks = Vec::with_capacity(config_urls.len());
        for config_url in config_urls {
            let client = self.clone();
            let config_url = config_url.clone();
            tasks.push(tokio::spawn(async move {
                client.live_playlists_from_url(&config_url).await
            }));
        }
        for task in tasks {
            let Ok(Ok(configured)) = task.await else {
                continue;
            };
            for playlist in configured {
                if seen.insert(playlist.url.clone()) {
                    playlists.push(playlist);
                }
            }
        }
        Ok(playlists)
    }

    async fn live_playlists_from_url(&self, config_url: &str) -> Result<Vec<LivePlaylist>> {
        let config: TvBoxConfig = self.get_config_json(config_url).await?;
        if let Some(urls) = config.urls {
            let mut playlists = Vec::new();
            for entry in urls.into_iter().take(MAX_CONFIG_INDEX_ENTRIES) {
                let Some(url) = normalized_https_url(&entry.url) else {
                    continue;
                };
                let Ok(nested) = self.get_config_json::<TvBoxConfig>(&url).await else {
                    continue;
                };
                playlists.extend(self.live_playlists_from_config(nested));
            }
            return Ok(playlists);
        }
        Ok(self.live_playlists_from_config(config))
    }

    fn live_playlists_from_config(&self, config: TvBoxConfig) -> Vec<LivePlaylist> {
        config
            .lives
            .into_iter()
            .filter(|live| live.r#type == 0)
            .filter_map(|live| {
                let url = normalized_live_url(&live.url)?;
                if self.broker.is_some() && !self.is_authorized(&url) {
                    return None;
                }
                Some(LivePlaylist {
                    name: if live.name.trim().is_empty() {
                        "直播".into()
                    } else {
                        clean_text(&live.name)
                    },
                    url,
                })
            })
            .collect()
    }

    async fn load_live_playlist(&self, playlist: &LivePlaylist) -> Result<Vec<Drama>> {
        let text = self
            .get_text_with_limit(&playlist.url, MAX_LIVE_RESPONSE_BYTES)
            .await?;
        Ok(parse_live_playlist(&text, &playlist.name))
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T> {
        self.get_json_with_limit(url, MAX_RESPONSE_BYTES).await
    }

    async fn get_config_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T> {
        let limit = if self.broker.is_some() {
            MAX_RESPONSE_BYTES
        } else {
            MAX_CONFIG_RESPONSE_BYTES
        };
        self.get_json_with_limit(url, limit).await
    }

    async fn get_json_with_limit<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        limit: usize,
    ) -> Result<T> {
        let bytes = self.get_bytes_with_limit(url, limit).await?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.trim_start_matches('\u{feff}').trim();
        serde_json::from_str(text).context("影视接口返回的不是有效 JSON")
    }

    async fn get_text_with_limit(&self, url: &str, limit: usize) -> Result<String> {
        let bytes = self.get_bytes_with_limit(url, limit).await?;
        Ok(String::from_utf8_lossy(&bytes)
            .trim_start_matches('\u{feff}')
            .to_owned())
    }

    async fn get_bytes_with_limit(&self, url: &str, limit: usize) -> Result<Vec<u8>> {
        let bytes = if let Some(broker) = &self.broker {
            ensure!(self.is_authorized(url), "影视源地址未被 AIO 授权");
            let request = self
                .client
                .post("http://localhost/egress/http")
                .header("x-aio-token", &broker.token)
                .header("x-aio-endpoint", url)
                .header("x-aio-method", "GET");
            response_bytes(request.send().await?, limit).await?
        } else {
            response_bytes(self.client.get(url).send().await?, limit).await?
        };
        Ok(bytes)
    }

    fn is_authorized(&self, url: &str) -> bool {
        self.broker.as_ref().is_none_or(|broker| {
            broker
                .http_endpoints
                .iter()
                .any(|allowed| same_http_endpoint(allowed, url))
        })
    }
}

async fn response_bytes(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let status = response.status();
    ensure!(status.is_success(), "影视接口返回 HTTP {}", status.as_u16());
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= limit,
            "影视接口响应超过 {} 字节",
            limit
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn normalized_api(api: &str) -> Option<String> {
    let mut url = Url::parse(api.trim()).ok()?;
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string().trim_end_matches('/').to_owned())
}

fn normalized_https_url(value: &str) -> Option<String> {
    let mut url = Url::parse(value.trim()).ok()?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

fn normalized_live_url(value: &str) -> Option<String> {
    let mut url = Url::parse(value.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    if matches!(url.host_str(), Some("127.0.0.1" | "localhost")) {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

fn with_query(base: &str, pairs: &[(&str, &str)]) -> Result<Url> {
    let mut url = Url::parse(base).context("影视源地址无效")?;
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in pairs {
            query.append_pair(name, value);
        }
    }
    Ok(url)
}

fn best_candidate(items: &[Vod], query: &str) -> Option<Vod> {
    let query = query.to_lowercase();
    let mut ranked = items
        .iter()
        .filter(|item| !item.vod_id.trim().is_empty() && !item.vod_name.trim().is_empty())
        .map(|item| {
            let title = item.vod_name.to_lowercase();
            let mut score = 0;
            if title == query {
                score += 100;
            }
            if title.contains(&query) {
                score += 50;
            }
            if query.contains(&title) {
                score += 30;
            }
            if title.contains('斗') && query.contains('斗') {
                score += 1;
            }
            (score, item)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    ranked
        .into_iter()
        .find(|(score, _)| *score > 0)
        .map(|(_, item)| item.clone())
}

fn best_class<'a>(classes: &'a [VodClass], category: &str) -> Option<&'a VodClass> {
    let level_one = |class: &VodClass| class.type_pid == "0" || class.type_pid.is_empty();
    let names: &[&str] = match category {
        "movie" => &["电影", "电影片"],
        "anime" => &["动漫", "动漫片", "动画", "番剧"],
        "series" => &["电视剧", "连续剧"],
        "variety" => &["综艺", "综艺片", "真人秀", "脱口秀"],
        "short" => &["短剧", "短剧大全", "爽文短剧", "漫剧"],
        _ => return None,
    };
    classes
        .iter()
        .filter(|class| level_one(class))
        .find(|class| names.iter().any(|name| class.type_name.trim() == *name))
        .or_else(|| {
            classes
                .iter()
                .filter(|class| level_one(class))
                .find(|class| {
                    let name = class.type_name.to_lowercase();
                    names.iter().any(|keyword| name.contains(keyword))
                })
        })
}

fn to_drama(vod: &Vod) -> Option<Drama> {
    let episodes = parse_episodes(&vod.vod_play_from, &vod.vod_play_url)?;
    let title = clean_text(&vod.vod_name);
    if title.is_empty() {
        return None;
    }
    let synopsis = clean_text(if vod.vod_blurb.trim().is_empty() {
        &vod.vod_content
    } else {
        &vod.vod_blurb
    });
    let genre = clean_text(if vod.type_name.trim().is_empty() {
        &vod.vod_class
    } else {
        &vod.type_name
    });
    let subtitle = clean_text(if vod.vod_remarks.trim().is_empty() {
        if vod.vod_sub.trim().is_empty() {
            "影视仓资源"
        } else {
            &vod.vod_sub
        }
    } else {
        &vod.vod_remarks
    });
    let tags = if genre.is_empty() {
        vec!["影视".into(), "点播".into()]
    } else {
        genre
            .split([',', '，', '/'])
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .take(5)
            .map(str::to_owned)
            .collect()
    };
    let poster = if vod.vod_pic.trim().starts_with("https://") {
        vod.vod_pic.trim().to_owned()
    } else {
        "assets/posters/forest-run.jpg".into()
    };
    let content_type = content_type(&genre, &tags);
    Some(Drama {
        id: format!("tvbox-{}", vod.vod_id.trim()),
        title,
        subtitle,
        synopsis: if synopsis.is_empty() {
            format!("{genre}，点击即可开始播放。")
        } else {
            synopsis
        },
        genre: if genre.is_empty() {
            "影视点播".into()
        } else {
            genre
        },
        content_type,
        mood: "热播".into(),
        duration_minutes: 0,
        poster: poster.clone(),
        backdrop: poster,
        accent: "#d7ff64".into(),
        tags,
        episodes,
    })
}

fn parse_count(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok()
}

fn parse_live_playlist(text: &str, playlist_name: &str) -> Vec<Drama> {
    let mut channels = Vec::new();
    let mut seen = HashSet::new();
    let mut pending_name = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') && !line.starts_with("#EXTINF") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            pending_name = rest
                .split_once(',')
                .map(|(_, title)| clean_live_name(title))
                .filter(|title| !title.is_empty());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some(url) = normalized_live_video(line) else {
            continue;
        };
        let title = pending_name
            .take()
            .unwrap_or_else(|| playlist_name.to_owned());
        if !seen.insert(format!("{title}|{url}")) {
            continue;
        }
        channels.push(Drama {
            id: format!("live-{}", channels.len() + 1),
            title: title.clone(),
            subtitle: format!("{playlist_name} · 直播"),
            synopsis: format!("来自{playlist_name}的直播频道。"),
            genre: "直播".into(),
            content_type: "live".into(),
            mood: "直播中".into(),
            duration_minutes: 0,
            poster: "assets/posters/forest-run.jpg".into(),
            backdrop: "assets/posters/forest-run.jpg".into(),
            accent: "#ff7a59".into(),
            tags: vec!["直播".into(), "频道".into()],
            episodes: vec![Episode {
                number: 1,
                title: "直播".into(),
                duration_seconds: 0,
                video: url,
                poster: String::new(),
            }],
        });
    }
    channels
}

fn normalized_live_video(value: &str) -> Option<String> {
    let value = value.trim();
    let url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    if matches!(url.host_str(), Some("127.0.0.1" | "localhost")) {
        return None;
    }
    Some(url.to_string())
}

fn clean_live_name(value: &str) -> String {
    let name = clean_text(value.split_once(',').map(|(_, name)| name).unwrap_or(value));
    if name.contains("更新时间") {
        String::new()
    } else {
        name
    }
}

fn content_type(genre: &str, tags: &[String]) -> String {
    let haystack = format!("{} {}", genre, tags.join(" ")).to_lowercase();
    if ["动漫", "动画", "国漫", "日漫", "番剧"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
    {
        "anime".into()
    } else if haystack.contains("电影") {
        "movie".into()
    } else if ["综艺", "真人秀", "脱口秀"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
    {
        "variety".into()
    } else if haystack.contains("短剧") {
        "short".into()
    } else {
        "series".into()
    }
}

fn parse_episodes(from: &str, urls: &str) -> Option<Vec<Episode>> {
    let routes = urls.split("$$$").collect::<Vec<_>>();
    let names = from.split("$$$").collect::<Vec<_>>();
    let mut best = Vec::new();
    for (index, route) in routes.iter().enumerate() {
        let route_name = names.get(index).copied().unwrap_or_default();
        let mut episodes = Vec::new();
        for (position, item) in route.split('#').enumerate() {
            let Some((label, url)) = item.split_once('$') else {
                continue;
            };
            let url = url.trim();
            if !is_direct_video(url) {
                continue;
            }
            let title = clean_text(label);
            episodes.push(Episode {
                number: (position + 1).min(u16::MAX as usize) as u16,
                title: if title.is_empty() {
                    format!("第{}集", position + 1)
                } else {
                    title
                },
                duration_seconds: 0,
                video: url.to_owned(),
                poster: String::new(),
            });
        }
        if episodes.len() > best.len() {
            best = episodes;
            if !route_name.trim().is_empty() && best.is_empty() {
                continue;
            }
        }
    }
    (!best.is_empty()).then_some(best)
}

fn is_direct_video(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    url.starts_with("https://") && (url.contains(".m3u8") || url.contains(".mp4"))
}

fn clean_text(value: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

fn default_searchable() -> i64 {
    1
}

fn string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(value) => value,
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    })
}

fn fallback_sources() -> Vec<Source> {
    [
        ("量子", "https://cj.lziapi.com/api.php/provide/vod"),
        ("极速", "https://jszyapi.com/api.php/provide/vod"),
        ("360", "https://360zy.com/api.php/provide/vod"),
        (
            "电影天堂",
            "https://caiji.dyttzyapi.com/api.php/provide/vod",
        ),
        ("非凡", "https://ffzy.tv/api.php/provide/vod"),
        ("豆瓣", "https://cdn.dzzyapi.com/api.php/provide/vod"),
        ("百度", "https://api.apibdzy.com/api.php/provide/vod"),
        ("光速", "https://api.guangsuapi.com/api.php/provide/vod"),
        ("暴风", "https://bfzyapi.com/api.php/provide/vod"),
        ("红牛", "https://www.hongniuzy2.com/api.php/provide/vod"),
    ]
    .into_iter()
    .map(|(name, api)| Source {
        name: name.into(),
        api: api.into(),
    })
    .collect()
}

fn fallback_live_playlists() -> Vec<LivePlaylist> {
    [
        ("虎牙一起看", "https://sub.ottiptv.cc/huyayqk.m3u"),
        (
            "综合直播",
            "https://down.nigx.cn/raw.githubusercontent.com/Kimentanm/aptv/master/m3u/iptv.m3u",
        ),
        ("综合电视", "https://raw.liucn.cc/box/libs/tv/tvlive.txt"),
        ("IPTV 直播", "https://z.szyyds.cn/iptv"),
    ]
    .into_iter()
    .map(|(name, url)| LivePlaylist {
        name: name.into(),
        url: url.into(),
    })
    .collect()
}

fn rerank_sources(sources: Vec<Source>, query: &str) -> Vec<Source> {
    let fallback = fallback_sources();
    let mut ranked = Vec::with_capacity(sources.len());
    let mut rest = sources;
    for preferred in fallback {
        if let Some(index) = rest.iter().position(|source| source.api == preferred.api) {
            ranked.push(rest.remove(index));
        }
    }
    ranked.extend(rest);
    if query.trim().is_empty() {
        return ranked;
    }
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_m3u8_route_and_skips_web_player_route() {
        let from = "liangzi$$$lzm3u8";
        let urls = "第01集$https://example.com/play/one#第02集$https://example.com/play/two$$$第01集$https://cdn.example.com/one/index.m3u8#第02集$https://cdn.example.com/two/index.m3u8";
        let episodes = parse_episodes(from, urls).expect("应解析出播放地址");
        assert_eq!(episodes.len(), 2);
        assert!(episodes[0].video.contains("index.m3u8"));
    }

    #[test]
    fn best_candidate_prefers_title_match() {
        let items = vec![
            test_vod("1", "斗破苍穹 年番"),
            test_vod("2", "斗破苍穹合集篇"),
        ];
        assert_eq!(best_candidate(&items, "斗破苍穹").unwrap().vod_id, "1");
    }

    #[test]
    fn broker_endpoint_matches_ignoring_query() {
        assert!(same_http_endpoint(
            "https://example.com/api.php/provide/vod",
            "https://example.com/api.php/provide/vod?ac=list&wd=test"
        ));
        assert!(!same_http_endpoint(
            "https://example.com/api.php/provide/vod",
            "https://example.com/api.php/provide/other?ac=list"
        ));
        assert!(!same_http_endpoint(
            "https://example.com/api.php/provide/vod",
            "https://other.example.com/api.php/provide/vod?ac=list"
        ));
    }

    fn test_vod(id: &str, name: &str) -> Vod {
        Vod {
            vod_id: id.into(),
            vod_name: name.into(),
            vod_sub: String::new(),
            vod_pic: String::new(),
            vod_remarks: String::new(),
            vod_class: String::new(),
            vod_content: String::new(),
            vod_blurb: String::new(),
            vod_play_from: String::new(),
            vod_play_url: String::new(),
            type_name: String::new(),
        }
    }
}
