use std::time::Duration;

use anyhow::{Context, Result, ensure};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{catalog, config::BrokerConfig};

const SYSTEM_PROMPT: &str = "你是电视点播意图解析器。只输出 JSON 对象，格式为 {\"query\":\"用户要搜索的片名或题材\",\"intent\":\"play|search|recommend\"}。去掉“我想看、播放、打开、来一部”等口语，只保留可检索的片名或题材。不要输出 Markdown，不要解释。";
const INTENT_TIMEOUT: Duration = Duration::from_secs(5);

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
            self.ensure_authorized_endpoint(&self.endpoint)?;
            self.client
                .post("http://localhost/egress")
                .header("x-aio-token", &broker.token)
                .header("x-aio-endpoint", &self.endpoint)
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

    pub(crate) async fn list_models(&self, secret: Option<&str>) -> Result<Vec<String>> {
        let key = secret
            .or(self.key.as_deref())
            .filter(|value| !value.trim().is_empty());
        let response = if let Some(broker) = &self.broker {
            self.ensure_authorized_endpoint(&self.endpoint)?;
            let mut request = self
                .client
                .get("http://localhost/egress/models")
                .header("x-aio-token", &broker.token)
                .header("x-aio-endpoint", &self.endpoint);
            if let Some(key) = key {
                request = request.bearer_auth(key);
            }
            request.send().await.context("读取模型列表失败")?
        } else {
            let mut request = self.client.get(format!("{}/models", self.endpoint));
            if let Some(key) = key {
                request = request.bearer_auth(key);
            }
            request.send().await.context("读取模型列表失败")?
        };
        ensure!(
            response.status().is_success(),
            "模型列表返回 HTTP {}",
            response.status().as_u16()
        );
        let bytes = limited_bytes(response, 2 * 1024 * 1024).await?;
        let catalog: ModelCatalog = serde_json::from_slice(&bytes).context("模型列表格式无效")?;
        let mut models = catalog
            .data
            .into_iter()
            .map(|model| model.id.trim().to_owned())
            .filter(|id| !id.is_empty() && id.len() <= 160)
            .collect::<Vec<_>>();
        models.sort();
        models.dedup();
        Ok(models)
    }

    pub(crate) async fn test_connection(&self, secret: Option<&str>) -> Result<()> {
        let key = secret
            .or(self.key.as_deref())
            .filter(|value| !value.trim().is_empty())
            .context("未配置 AI Key")?;
        let payload = serde_json::json!({
            "model": self.model,
            "stream": false,
            "temperature": 0,
            "max_tokens": 8,
            "messages": [
                {"role": "user", "content": "只回复 OK"}
            ]
        });
        let response = if let Some(broker) = &self.broker {
            self.ensure_authorized_endpoint(&self.endpoint)?;
            self.client
                .post("http://localhost/egress")
                .header("x-aio-token", &broker.token)
                .header("x-aio-endpoint", &self.endpoint)
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .context("模型连接测试失败")?
        } else {
            self.client
                .post(format!("{}/chat/completions", self.endpoint))
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .context("模型连接测试失败")?
        };
        ensure!(
            response.status().is_success(),
            "模型连接测试返回 HTTP {}",
            response.status().as_u16()
        );
        let bytes = limited_bytes(response, 2 * 1024 * 1024).await?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).context("模型响应格式无效")?;
        ensure!(
            value["choices"]
                .as_array()
                .is_some_and(|choices| !choices.is_empty()),
            "模型没有返回推理结果"
        );
        Ok(())
    }

    fn ensure_authorized_endpoint(&self, endpoint: &str) -> Result<()> {
        let Some(broker) = &self.broker else {
            return Ok(());
        };
        ensure!(
            broker
                .model_endpoints
                .iter()
                .any(|allowed| allowed == endpoint),
            "模型地址未被 AIO 授权"
        );
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct ModelCatalog {
    #[serde(default)]
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ModelTestRequest {
    pub model_endpoint: String,
    pub model: String,
    #[serde(default)]
    pub secret: Option<String>,
}

pub(crate) async fn understand(message: &str, ai: &AiClient) -> Result<ParsedIntent> {
    match tokio::time::timeout(INTENT_TIMEOUT, ai.parse(message)).await {
        Ok(Ok(parsed)) => return Ok(parsed),
        Ok(Err(error)) => eprintln!("AI 意图解析失败，使用规则解析: {error:#}"),
        Err(_) => eprintln!("AI 意图解析超时，使用规则解析"),
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

async fn read_stream_response(response: reqwest::Response) -> Result<Vec<u8>> {
    limited_bytes(response, 2 * 1024 * 1024).await
}

async fn limited_bytes(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(bytes.len() + chunk.len() <= limit, "AI 响应超过配额");
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
