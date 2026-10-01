use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Drama {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub synopsis: &'static str,
    pub genre: &'static str,
    pub mood: &'static str,
    pub duration_minutes: u16,
    pub poster: &'static str,
    pub backdrop: &'static str,
    pub accent: &'static str,
    pub tags: &'static [&'static str],
    pub episodes: &'static [Episode],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Episode {
    pub number: u16,
    pub title: &'static str,
    pub duration_seconds: u16,
    pub video: &'static str,
    pub poster: &'static str,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CatalogQuery {
    pub q: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CatalogView {
    pub featured: Drama,
    pub items: Vec<Drama>,
}

const FOREST_EPISODES: &[Episode] = &[Episode {
    number: 1,
    title: "误入森林",
    duration_seconds: 33,
    video: "assets/videos/forest-run.mp4",
    poster: "assets/posters/forest-run.jpg",
}];

const LLAMA_EPISODES: &[Episode] = &[Episode {
    number: 1,
    title: "雪山相遇",
    duration_seconds: 45,
    video: "assets/videos/llama-drama.mp4",
    poster: "assets/posters/llama-drama.jpg",
}];

const COFFEE_EPISODES: &[Episode] = &[Episode {
    number: 1,
    title: "晨间奇遇",
    duration_seconds: 40,
    video: "assets/videos/coffee-run.mp4",
    poster: "assets/posters/coffee-run.jpg",
}];

pub(crate) const DRAMAS: &[Drama] = &[
    Drama {
        id: "forest-run",
        title: "森林狂奔",
        subtitle: "一只兔子的反击",
        synopsis: "安静的森林被三个捣蛋鬼打破，主角决定用机智守住自己的家园。轻松、明快，适合全家一起看。",
        genre: "动画喜剧",
        mood: "轻松",
        duration_minutes: 1,
        poster: "assets/posters/forest-run.jpg",
        backdrop: "assets/posters/forest-run.jpg",
        accent: "#d7ff64",
        tags: &["动物", "冒险", "喜剧"],
        episodes: FOREST_EPISODES,
    },
    Drama {
        id: "llama-drama",
        title: "雪原小羊驼",
        subtitle: "横穿荒原的旅程",
        synopsis: "一只好奇心旺盛的小羊驼踏上雪山旅程，在辽阔天地里遇见意外伙伴，也学会面对自己的胆怯。",
        genre: "冒险动画",
        mood: "治愈",
        duration_minutes: 1,
        poster: "assets/posters/llama-drama.jpg",
        backdrop: "assets/posters/llama-drama.jpg",
        accent: "#ff9c5a",
        tags: &["动物", "治愈", "冒险"],
        episodes: LLAMA_EPISODES,
    },
    Drama {
        id: "coffee-run",
        title: "咖啡奇旅",
        subtitle: "把清晨跑成一场电影",
        synopsis: "咖啡、城市和一段不断加速的清晨。看似普通的一天，因为一次奔跑变成了充满想象力的旅程。",
        genre: "都市奇想",
        mood: "轻快",
        duration_minutes: 1,
        poster: "assets/posters/coffee-run.jpg",
        backdrop: "assets/posters/coffee-run.jpg",
        accent: "#6fd6ff",
        tags: &["都市", "奇想", "轻快"],
        episodes: COFFEE_EPISODES,
    },
];

pub(crate) fn catalog(query: Option<&str>) -> CatalogView {
    let normalized = query.unwrap_or_default().trim().to_lowercase();
    let items = if normalized.is_empty() {
        DRAMAS.to_vec()
    } else {
        let matched: Vec<_> = DRAMAS
            .iter()
            .filter(|drama| matches(drama, &normalized))
            .cloned()
            .collect();
        if matched.is_empty() {
            DRAMAS.to_vec()
        } else {
            matched
        }
    };
    CatalogView {
        featured: items[0].clone(),
        items,
    }
}

fn matches(drama: &Drama, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {}",
        drama.title,
        drama.subtitle,
        drama.synopsis,
        drama.genre,
        drama.mood,
        drama.tags.join(" ")
    )
    .to_lowercase();
    query.split_whitespace().all(|term| haystack.contains(term))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_returns_matching_drama() {
        let view = catalog(Some("动物 治愈"));
        assert_eq!(view.featured.id, "llama-drama");
        assert_eq!(view.items.len(), 1);
    }

    #[test]
    fn empty_search_keeps_full_catalog() {
        let view = catalog(None);
        assert_eq!(view.items.len(), DRAMAS.len());
    }

    #[test]
    fn unknown_search_falls_back_to_featured_catalog() {
        let view = catalog(Some("不存在的题材"));
        assert_eq!(view.items.len(), DRAMAS.len());
    }
}
