use std::time::Duration;

use anyhow::{Context, Result, ensure};
use reqwest::Client;
use serde::Deserialize;

use crate::{catalog, config::BrokerConfig};

const SYSTEM_PROMPT: &str = "你是电视点播意图解析器。只输出 JSON 对象，格式为 {\"query\":\"用户要搜索的片名或题材\",\"intent\":\"play|search|recommend\"}。去掉“我想看、播放、打开、来一部”等口语，只保留可检索的片名或题材。不要输出 Markdown，不要解释。";

#[derive(Clone)]
pub(crate) struct AiClient {
    client: Client,
    endpoint: String,
    model: String,
    key: Option<String>,
    broker: Option<BrokerConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ParsedIntent {
    pub query: String,
    pub intent: String,
}

impl AiClient {
    pub(crate) fn new(
        endpoint: String,
        model: String,
        key: Option<String>,
        broker: Option<BrokerConfig>,
    ) -> Result<Self> {
        let mut builder = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(35));
        if let Some(broker) = &broker {
            builder = builder.unix_socket(broker.socket.clone());
        }
        let client = builder.build()?;
        Ok(Self {
            client,
            endpoint,
            model,
            key,
            broker,
        })
    }

    pub(crate) async fn parse(&self, message: &str) -> Result<ParsedIntent> {
        let Some(key) = self.key.as_deref() else {
            return Err(anyhow::anyhow!("未配置 AI Key"));
        };
        let payload = serde_json::json!({
            "model": self.model,
            "stream": self.broker.is_some(),
            "temperature": 0,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": message}
            ]
        });
        let response = if let Some(broker) = &self.broker {
            self.client
                .post("http://localhost/egress")
                .header("x-aio-token", &broker.token)
                .header("x-aio-endpoint", &broker.model_endpoint)
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .context("AI 意图解析失败")?
        } else {
            self.client
                .post(format!("{}/chat/completions", self.endpoint))
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .context("AI 意图解析失败")?
        };
        ensure!(
            response.status().is_success(),
            "AI 服务返回 HTTP {}",
            response.status().as_u16()
        );
        let bytes = if self.broker.is_some() {
            read_stream_response(response).await?
        } else {
            response.bytes().await?.to_vec()
        };
        parse_chat_response(&bytes)
    }
}

pub(crate) async fn understand(message: &str, ai: &AiClient) -> Result<ParsedIntent> {
    match ai.parse(message).await {
        Ok(parsed) => return Ok(parsed),
        Err(error) => eprintln!("AI 意图解析失败，使用规则解析: {error:#}"),
    }
    let fallback = catalog::fallback_dramas()
        .into_iter()
        .find(|drama| message.to_lowercase().contains(&drama.title.to_lowercase()));
    if let Some(drama) = fallback {
        return Ok(ParsedIntent {
            query: drama.title,
            intent: "play".into(),
        });
    }
    Ok(rule_based_intent(message))
}

fn rule_based_intent(message: &str) -> ParsedIntent {
    let normalized = message.trim();
    let intent = if ["播放", "看看", "想看", "来一部", "打开", "开始", "给我看"]
        .iter()
        .any(|keyword| normalized.contains(keyword))
    {
        "play"
    } else if normalized.contains("推荐") {
        "recommend"
    } else {
        "search"
    };
    let mut query = normalized.to_owned();
    for prefix in [
        "我想看",
        "我要看",
        "想看",
        "播放",
        "看看",
        "打开",
        "开始",
        "来一部",
        "给我看",
    ] {
        query = query.replace(prefix, "");
    }
    for suffix in ["短剧", "电视剧", "电影", "动漫", "动画"] {
        if query.trim() == suffix {
            query.clear();
            break;
        }
    }
    ParsedIntent {
        query: query.trim().to_owned(),
        intent: intent.into(),
    }
}

fn parse_chat_response(bytes: &[u8]) -> Result<ParsedIntent> {
    let text = String::from_utf8_lossy(bytes);
    let mut content = String::new();
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" || data.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(delta) = value["choices"][0]["delta"]["content"].as_str() {
            content.push_str(delta);
        }
    }
    let value = if content.is_empty() {
        serde_json::from_slice::<serde_json::Value>(bytes)?
    } else {
        serde_json::from_str::<serde_json::Value>(&content)?
    };
    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .or_else(|| value["choices"][0]["text"].as_str())
        .or_else(|| value["content"].as_str())
        .unwrap_or(content.as_str());
    let parsed: ParsedIntent =
        serde_json::from_str(content.trim()).context("AI 返回的意图 JSON 无效")?;
    ensure!(!parsed.query.trim().is_empty(), "AI 未返回搜索关键词");
    Ok(parsed)
}

async fn read_stream_response(mut response: reqwest::Response) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 2 * 1024 * 1024,
            "AI 响应超过配额"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_query_from_natural_language() {
        let parsed = rule_based_intent("我想看斗破苍穹");
        assert_eq!(parsed.query, "斗破苍穹");
        assert_eq!(parsed.intent, "play");
    }

    #[test]
    fn parses_streamed_chat_content() {
        let bytes = "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"query\\\":\\\"斗破苍穹\\\",\"}}]}\ndata: {\"choices\":[{\"delta\":{\"content\":\"\\\"intent\\\":\\\"play\\\"}\"}}]}\ndata: [DONE]\n".as_bytes();
        let parsed = parse_chat_response(bytes).unwrap();
        assert_eq!(parsed.query, "斗破苍穹");
        assert_eq!(parsed.intent, "play");
    }
}
