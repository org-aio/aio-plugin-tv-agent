use std::time::Duration;

use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url};
use serde::{Deserialize, Deserializer};

use crate::{
    catalog::{Drama, Episode},
    config::BrokerConfig,
};

const MAX_RESPONSE_BYTES: usize = 512_000;

#[derive(Clone)]
pub(crate) struct TvBoxClient {
    client: Client,
    config_url: String,
    broker: Option<BrokerConfig>,
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
    pub(crate) fn new(config_url: String, broker: Option<BrokerConfig>) -> Result<Self> {
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
            config_url,
            broker,
        })
    }

    pub(crate) async fn search(&self, query: &str) -> Result<Option<Drama>> {
        let query = query.trim();
        ensure!(!query.is_empty(), "搜索关键词不能为空");
        let sources = self.sources().await?;
        let mut last_error = None;

        for source in sources.into_iter().take(20) {
            match self.search_source(&source, query).await {
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

    async fn sources(&self) -> Result<Vec<Source>> {
        let config: TvBoxConfig = self
            .get_json(&self.config_url)
            .await
            .context("读取 TVBox 配置失败")?;
        let mut sources = Vec::new();
        for site in config.sites {
            if site.r#type != 1 || site.searchable == 0 {
                continue;
            }
            let Some(api) = normalized_api(&site.api) else {
                continue;
            };
            if !api.starts_with("https://") {
                continue;
            }
            if !self.is_authorized(&api) {
                continue;
            }
            let name = if site.name.trim().is_empty() {
                site.key
            } else {
                site.name
            };
            sources.push(Source { name, api });
        }
        for source in fallback_sources() {
            if self.is_authorized(&source.api)
                && !sources
                    .iter()
                    .any(|configured| configured.api == source.api)
            {
                sources.push(source);
            }
        }
        Ok(sources)
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
        let Some(drama) = to_drama(&vod, &source.name) else {
            return Ok(None);
        };
        Ok(Some(drama))
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T> {
        let bytes = if let Some(broker) = &self.broker {
            ensure!(self.is_authorized(url), "影视源地址未被 AIO 授权");
            let request = self
                .client
                .post("http://localhost/egress/http")
                .header("x-aio-token", &broker.token)
                .header("x-aio-endpoint", url)
                .header("x-aio-method", "GET");
            response_bytes(request.send().await?).await?
        } else {
            response_bytes(self.client.get(url).send().await?).await?
        };
        let text = String::from_utf8_lossy(&bytes);
        let text = text.trim_start_matches('\u{feff}').trim();
        serde_json::from_str(text).context("影视接口返回的不是有效 JSON")
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

async fn response_bytes(response: reqwest::Response) -> Result<Vec<u8>> {
    let status = response.status();
    ensure!(status.is_success(), "影视接口返回 HTTP {}", status.as_u16());
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= MAX_RESPONSE_BYTES,
            "影视接口响应超过 512 KB"
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

fn same_http_endpoint(allowed: &str, requested: &str) -> bool {
    let Ok(allowed) = Url::parse(allowed) else {
        return false;
    };
    let Ok(requested) = Url::parse(requested) else {
        return false;
    };
    allowed.scheme() == requested.scheme()
        && allowed.host_str() == requested.host_str()
        && allowed.port_or_known_default() == requested.port_or_known_default()
        && allowed.path().trim_end_matches('/') == requested.path().trim_end_matches('/')
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

fn to_drama(vod: &Vod, source_name: &str) -> Option<Drama> {
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
    Some(Drama {
        id: format!("tvbox-{}", vod.vod_id.trim()),
        title,
        subtitle,
        synopsis: if synopsis.is_empty() {
            format!("来自{source_name}的影视资源，点击即可开始播放。")
        } else {
            synopsis
        },
        genre: if genre.is_empty() {
            "影视点播".into()
        } else {
            genre
        },
        mood: "热播".into(),
        duration_minutes: 0,
        poster: poster.clone(),
        backdrop: poster,
        accent: "#d7ff64".into(),
        tags,
        episodes,
    })
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
        ("百度", "https://api.apibdzy.com/api.php/provide/vod"),
        ("暴风", "https://bfzyapi.com/api.php/provide/vod"),
        ("红牛", "https://www.hongniuzy2.com/api.php/provide/vod"),
        ("光速", "https://api.guangsuapi.com/api.php/provide/vod"),
    ]
    .into_iter()
    .map(|(name, api)| Source {
        name: name.into(),
        api: api.into(),
    })
    .collect()
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
