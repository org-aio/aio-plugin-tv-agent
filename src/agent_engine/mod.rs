use serde::{Deserialize, Serialize};

use crate::catalog::{self, Drama, Episode};

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

#[derive(Clone, Copy, Debug, Serialize)]
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

pub(crate) fn respond(request: AgentRequest) -> AgentReply {
    let normalized = request.message.trim().to_lowercase();
    if normalized.is_empty() {
        return reply(
            Intent::Recommend,
            "你可以告诉我想看轻松、治愈、动物或都市题材，我会直接帮你播放。",
            vec!["来一部治愈的动物短剧".into(), "播放咖啡奇旅".into()],
            None,
        );
    }

    let wants_play = ["播放", "看看", "想看", "来一部", "打开", "开始"]
        .iter()
        .any(|keyword| normalized.contains(keyword));
    if let Some(drama) = find_explicit_drama(&normalized) {
        let message = format!(
            "已为你选中《{}》，第 {} 集。",
            drama.title, drama.episodes[0].number
        );
        let selection = selection(drama);
        let intent = if wants_play {
            Intent::Play
        } else {
            Intent::Search
        };
        return reply(intent, &message, suggestions(drama), Some(selection));
    }

    let ranked = rank(&normalized);
    if let Some(drama) = ranked.first() {
        let message = if wants_play {
            format!("找到适合你的《{}》，现在开始播放。", drama.title)
        } else {
            format!("我为你挑选了《{}》，{}.", drama.title, drama.subtitle)
        };
        let intent = if wants_play {
            Intent::Play
        } else {
            Intent::Recommend
        };
        return reply(intent, &message, suggestions(drama), Some(selection(drama)));
    }

    reply(
        Intent::Search,
        "暂时没有匹配的短剧，可以试试动物、治愈、冒险或轻松题材。",
        vec!["推荐动物短剧".into(), "播放森林狂奔".into()],
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

fn selection(drama: &Drama) -> Selection {
    Selection {
        drama: drama.clone(),
        episode: drama.episodes[0].clone(),
    }
}

fn suggestions(drama: &Drama) -> Vec<String> {
    vec![
        format!("播放{}", drama.title),
        format!("还有{}这样的短剧吗", drama.genre),
    ]
}

fn find_explicit_drama(query: &str) -> Option<&'static Drama> {
    catalog::DRAMAS
        .iter()
        .find(|drama| query.contains(&drama.title.to_lowercase()))
}

fn rank(query: &str) -> Vec<&'static Drama> {
    let mut scored: Vec<_> = catalog::DRAMAS
        .iter()
        .filter_map(|drama| {
            let score = score(drama, query);
            (score > 0).then_some((score, drama))
        })
        .collect();
    scored.sort_by(|left, right| right.0.cmp(&left.0));
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
        let result = respond(AgentRequest {
            message: "播放森林狂奔".into(),
        });
        let selection = result.selection.expect("应返回选集");
        assert!(matches!(result.intent, Intent::Play));
        assert_eq!(selection.drama.id, "forest-run");
        assert_eq!(selection.episode.video, "assets/videos/forest-run.mp4");
    }

    #[test]
    fn mood_request_ranks_healing_animal_drama() {
        let result = respond(AgentRequest {
            message: "我想看治愈的动物短剧".into(),
        });
        assert_eq!(
            result.selection.expect("应返回推荐").drama.id,
            "llama-drama"
        );
    }

    #[test]
    fn empty_message_returns_recommendations() {
        let result = respond(AgentRequest {
            message: "  ".into(),
        });
        assert!(matches!(result.intent, Intent::Recommend));
        assert!(result.selection.is_none());
        assert_eq!(result.suggestions.len(), 2);
    }
}
