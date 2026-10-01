use serde::{Deserialize, Serialize};

use crate::{
    ai::{AiClient, understand},
    catalog::{self, Drama, Episode},
    tvbox::TvBoxClient,
};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AgentRequest {
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AgentReply {
    pub intent: Intent,
    pub message: String,
    pub suggestions: Vec<String>,
    pub selection: Option<Selection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Intent {
    Recommend,
    Play,
    Search,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Selection {
    pub drama: Drama,
    pub episode: Episode,
}

pub(crate) async fn respond(
    request: AgentRequest,
    ai: &AiClient,
    tvbox: &TvBoxClient,
) -> AgentReply {
    let message = request.message.trim();
    if message.is_empty() {
        return reply(
            Intent::Recommend,
            "你可以告诉我想看轻松、治愈、动物或都市题材，我会直接帮你播放。",
            vec!["来一部治愈的动物短剧".into(), "播放咖啡奇旅".into()],
            None,
        );
    }

    let parsed = understand(message, ai)
        .await
        .unwrap_or_else(|_| crate::ai::ParsedIntent {
            query: message.to_owned(),
            intent: "search".into(),
        });
    let intent = match parsed.intent.as_str() {
        "play" => Intent::Play,
        "recommend" => Intent::Recommend,
        _ => Intent::Search,
    };
    let query = parsed.query.trim();
    if !query.is_empty() {
        match tvbox.search(query).await {
            Ok(Some(drama)) => {
                if let Some(episode) = drama.first_selection() {
                    let text = if intent == Intent::Play {
                        format!(
                            "找到《{}》，共 {} 集。请选择要播放的集数。",
                            drama.title,
                            drama.episodes.len()
                        )
                    } else {
                        format!("找到《{}》，{}。", drama.title, drama.subtitle)
                    };
                    return reply(
                        intent,
                        &text,
                        suggestions(&drama),
                        Some(Selection { drama, episode }),
                    );
                }
            }
            Ok(None) => eprintln!("影视搜索没有匹配: {query}"),
            Err(error) => eprintln!("影视搜索失败 {query}: {error:#}"),
        }
    }

    let normalized = message.to_lowercase();
    if let Some(drama) = find_explicit_drama(&normalized) {
        let episode = drama.episodes[0].clone();
        let text = format!("已为你选中《{}》，第 {} 集。", drama.title, episode.number);
        return reply(
            if intent == Intent::Play {
                Intent::Play
            } else {
                Intent::Search
            },
            &text,
            suggestions(&drama),
            Some(Selection { drama, episode }),
        );
    }

    let ranked = rank(&normalized);
    if let Some(drama) = ranked.first() {
        let episode = drama.episodes[0].clone();
        let text = if intent == Intent::Play {
            format!(
                "找到适合你的《{}》，共 {} 集。请选择要播放的集数。",
                drama.title,
                drama.episodes.len()
            )
        } else {
            format!("我为你挑选了《{}》，{}。", drama.title, drama.subtitle)
        };
        let actual_intent = if intent == Intent::Play {
            Intent::Play
        } else {
            Intent::Recommend
        };
        return reply(
            actual_intent,
            &text,
            suggestions(drama),
            Some(Selection {
                drama: drama.clone(),
                episode,
            }),
        );
    }

    reply(
        Intent::Search,
        "暂时没有匹配的影视资源，可以试试更直接的片名，例如“我想看斗破苍穹”。",
        vec!["我想看斗破苍穹".into(), "播放森林狂奔".into()],
        None,
    )
}

fn reply(
    intent: Intent,
    message: &str,
    suggestions: Vec<String>,
    selection: Option<Selection>,
) -> AgentReply {
    AgentReply {
        intent,
        message: message.to_owned(),
        suggestions,
        selection,
    }
}

fn suggestions(drama: &Drama) -> Vec<String> {
    vec![
        format!("播放{}", drama.title),
        format!("还有{}这样的影视吗", drama.genre),
    ]
}

fn find_explicit_drama(query: &str) -> Option<Drama> {
    catalog::fallback_dramas()
        .into_iter()
        .find(|drama| query.contains(&drama.title.to_lowercase()))
}

fn rank(query: &str) -> Vec<Drama> {
    let mut scored: Vec<_> = catalog::fallback_dramas()
        .into_iter()
        .filter_map(|drama| {
            let score = score(&drama, query);
            (score > 0).then_some((score, drama))
        })
        .collect();
    scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    scored.into_iter().map(|(_, drama)| drama).collect()
}

fn score(drama: &Drama, query: &str) -> usize {
    let mut score = 0;
    for keyword in keywords(query) {
        if matches!(keyword, "短剧" | "动画" | "搞笑") {
            continue;
        }
        if drama.title.to_lowercase().contains(keyword) {
            score += 8;
        }
        if drama.subtitle.to_lowercase().contains(keyword) {
            score += 6;
        }
        if drama.genre.to_lowercase().contains(keyword) {
            score += 5;
        }
        if drama.mood.to_lowercase().contains(keyword) {
            score += 4;
        }
        if drama
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(keyword))
        {
            score += 3;
        }
    }
    score
}

fn keywords(query: &str) -> Vec<&str> {
    let mut output = Vec::new();
    for word in query.split_whitespace() {
        output.push(word);
    }
    for keyword in [
        "轻松", "搞笑", "治愈", "动物", "冒险", "喜剧", "都市", "奇想", "短剧", "动画",
    ] {
        if query.contains(keyword) {
            output.push(keyword);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_play_request_selects_episode() {
        let drama = find_explicit_drama("播放森林狂奔").expect("应找到演示短剧");
        let episode = drama.first_selection().expect("应有剧集");
        assert_eq!(drama.id, "forest-run");
        assert_eq!(episode.video, "assets/videos/forest-run.mp4");
    }

    #[test]
    fn mood_request_ranks_healing_animal_drama() {
        let drama = rank("我想看治愈的动物短剧").remove(0);
        assert_eq!(drama.id, "llama-drama");
    }
}
