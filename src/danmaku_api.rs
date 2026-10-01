use std::time::Duration;

use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::BrokerConfig;

const MAX_RESPONSE_BYTES: usize = 512_000;

#[derive(Debug, Deserialize)]
struct DanmakuEntry {
    #[serde(default)]
    url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DanmakuItem {
    pub(crate) time: f64,
    #[serde(rename = "type")]
    pub(crate) kind: i32,
    pub(crate) size: i32,
    pub(crate) color: i32,
    pub(crate) text: String,
}

pub(crate) async fn load(
    template: &str,
    title: &str,
    episode: &str,
    broker: Option<BrokerConfig>,
) -> Result<Vec<DanmakuItem>> {
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20));
    if let Some(broker) = &broker {
        builder = builder.unix_socket(broker.socket.clone());
    }
    let client = builder.build()?;
    let response = if template.contains("{name}") || template.contains("{episode}") {
        let url = template
            .replace("{name}", &encode(title))
            .replace("{episode}", &encode(episode));
        get_text(&client, &url, broker.as_ref()).await?
    } else {
        post_text(&client, template, title, episode, broker.as_ref()).await?
    };
    let text = response.trim();
    ensure!(!text.is_empty(), "弹幕接口返回为空");
    if text.starts_with("https://") {
        return parse(&get_bytes(&client, text, broker.as_ref()).await?);
    }
    if text.starts_with('[') {
        let value: Value = serde_json::from_str(text).context("弹幕索引格式无效")?;
        if looks_like_danmaku_array(&value) {
            return parse(text.as_bytes());
        }
        let entries: Vec<DanmakuEntry> = serde_json::from_str(text).context("弹幕索引格式无效")?;
        let Some(entry) = entries
            .into_iter()
            .find(|entry| !entry.url.trim().is_empty())
        else {
            return Ok(Vec::new());
        };
        return parse(&get_bytes(&client, entry.url.trim(), broker.as_ref()).await?);
    }
    if text.starts_with('{') {
        let value: Value = serde_json::from_str(text).context("弹幕索引格式无效")?;
        if let Some(url) = value.get("url").and_then(Value::as_str) {
            return parse(&get_bytes(&client, url, broker.as_ref()).await?);
        }
    }
    parse(text.as_bytes())
}

async fn get_text(client: &Client, url: &str, broker: Option<&BrokerConfig>) -> Result<String> {
    Ok(String::from_utf8_lossy(&get_bytes(client, url, broker).await?).into_owned())
}

async fn post_text(
    client: &Client,
    url: &str,
    title: &str,
    episode: &str,
    broker: Option<&BrokerConfig>,
) -> Result<String> {
    if broker.is_some() {
        anyhow::bail!("AIO 安装模式的弹幕接口必须使用 name 或 episode 的 GET 模板变量");
    }
    let response = client
        .post(url)
        .header(
            header::CONTENT_TYPE,
            "application/x-www-form-urlencoded; charset=UTF-8",
        )
        .body(format!(
            "name={}&episode={}",
            encode(title),
            encode(episode)
        ))
        .send()
        .await?;
    let bytes = response_bytes(response, MAX_RESPONSE_BYTES).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn get_bytes(client: &Client, url: &str, broker: Option<&BrokerConfig>) -> Result<Vec<u8>> {
    let response = if let Some(broker) = broker {
        ensure_authorized(broker, url)?;
        client
            .post("http://localhost/egress/http")
            .header("x-aio-token", &broker.token)
            .header("x-aio-endpoint", url)
            .header("x-aio-method", "GET")
            .header(header::USER_AGENT, "AIO-TV-Agent/0.1")
            .send()
            .await?
    } else {
        client
            .get(url)
            .header(header::USER_AGENT, "AIO-TV-Agent/0.1")
            .send()
            .await?
    };
    response_bytes(response, MAX_RESPONSE_BYTES).await
}

async fn response_bytes(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let status = response.status();
    ensure!(status.is_success(), "弹幕接口返回 HTTP {}", status.as_u16());
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= limit,
            "弹幕响应超过 {limit} 字节"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn ensure_authorized(broker: &BrokerConfig, requested: &str) -> Result<()> {
    let requested = Url::parse(requested).context("弹幕数据地址无效")?;
    ensure!(
        broker.http_endpoints.iter().any(|allowed| {
            Url::parse(allowed).is_ok_and(|allowed| {
                allowed.scheme() == requested.scheme()
                    && allowed.host_str() == requested.host_str()
                    && allowed.port_or_known_default() == requested.port_or_known_default()
                    && allowed.path() == requested.path()
            })
        }),
        "弹幕接口未被 AIO 授权"
    );
    Ok(())
}

fn looks_like_danmaku_array(value: &Value) -> bool {
    value.as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item.get("time").is_some()
                || item.get("t").is_some()
                || item.get("text").is_some()
                || item.get("content").is_some()
        })
    })
}

fn parse(bytes: &[u8]) -> Result<Vec<DanmakuItem>> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}').trim();
    let mut result = Vec::new();
    if text.starts_with('[') {
        let values: Vec<Value> = serde_json::from_str(text).context("弹幕数据不是有效 JSON")?;
        for value in values {
            let text = value
                .get("text")
                .or_else(|| value.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if text.is_empty() {
                continue;
            }
            let time = value
                .get("time")
                .or_else(|| value.get("t"))
                .and_then(Value::as_f64)
                .unwrap_or_default();
            result.push(DanmakuItem {
                time: if time < 100_000.0 {
                    time
                } else {
                    time / 1000.0
                },
                kind: 1,
                size: 22,
                color: 0xFFFFFF,
                text: text.to_owned(),
            });
        }
        return Ok(result);
    }
    for line in text.lines() {
        if line.contains("<d ") && line.contains("p=\"") {
            if let Some(item) = parse_xml_item(line) {
                result.push(item);
            }
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.starts_with('[') {
            continue;
        }
        let Some((params, content)) = split_bracket(trimmed) else {
            continue;
        };
        let values = params.split(',').collect::<Vec<_>>();
        if values.len() < 4 {
            continue;
        }
        let Ok(time) = values[0].parse::<f64>() else {
            continue;
        };
        let kind = values[1].parse::<i32>().unwrap_or(1);
        let size = values
            .get(2)
            .and_then(|value| value.parse().ok())
            .unwrap_or(22);
        let color = values
            .get(3)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0xFFFFFF);
        let text = content.trim();
        if text.is_empty() {
            continue;
        }
        result.push(DanmakuItem {
            time,
            kind,
            size,
            color,
            text: text.to_owned(),
        });
    }
    Ok(result)
}

fn parse_xml_item(line: &str) -> Option<DanmakuItem> {
    let start = line.find("p=\"")? + 3;
    let end = line[start..].find('"')? + start;
    let params = &line[start..end];
    let content_start = line[end..].find('>')? + end + 1;
    let content_end = line[content_start..].find("</d>")? + content_start;
    let values = params.split(',').collect::<Vec<_>>();
    if values.len() < 4 {
        return None;
    }
    let time = values[0].parse::<f64>().ok()?;
    let kind = values[1].parse::<i32>().unwrap_or(1);
    let size = values
        .get(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(22);
    let color = values
        .get(3)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0xFFFFFF);
    let text = line[content_start..content_end]
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(DanmakuItem {
        time,
        kind,
        size,
        color,
        text: text.to_owned(),
    })
}

fn split_bracket(value: &str) -> Option<(&str, &str)> {
    let end = value.find(']')?;
    Some((&value[1..end], &value[end + 1..]))
}

fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xml_and_bracket_payloads() {
        let items =
            parse(b"<i><d p=\"1.5,1,25,16777215,0,0,0\">hello</d>\n[2.5,5,22,16711680]world\0</i>")
                .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "hello");
        assert_eq!(items[0].kind, 1);
        assert_eq!(items[1].kind, 5);
        assert_eq!(items[1].color, 0xFF0000);
    }
}
