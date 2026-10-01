use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Drama {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub synopsis: String,
    pub genre: String,
    pub mood: String,
    pub duration_minutes: u16,
    pub poster: String,
    pub backdrop: String,
    pub accent: String,
    pub tags: Vec<String>,
    pub episodes: Vec<Episode>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Episode {
    pub number: u16,
    pub title: String,
    pub duration_seconds: u16,
    pub video: String,
    pub poster: String,
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

impl Drama {
    pub(crate) fn first_selection(&self) -> Option<Episode> {
        self.episodes.first().cloned()
    }
}

pub(crate) fn catalog(query: Option<&str>) -> CatalogView {
    let normalized = query.unwrap_or_default().trim().to_lowercase();
    let items = if normalized.is_empty() {
        fallback_dramas()
    } else {
        let matched: Vec<_> = fallback_dramas()
            .into_iter()
            .filter(|drama| matches(drama, &normalized))
            .collect();
        if matched.is_empty() {
            fallback_dramas()
        } else {
            matched
        }
    };
    CatalogView {
        featured: items[0].clone(),
        items,
    }
}

pub(crate) fn fallback_dramas() -> Vec<Drama> {
    vec![
        Drama {
            id: "forest-run".into(),
            title: "森林狂奔".into(),
            subtitle: "一只兔子的反击".into(),
            synopsis: "安静的森林被三个捣蛋鬼打破，主角决定用机智守住自己的家园。轻松、明快，适合全家一起看。".into(),
            genre: "动画喜剧".into(),
            mood: "轻松".into(),
            duration_minutes: 1,
            poster: "assets/posters/forest-run.jpg".into(),
            backdrop: "assets/posters/forest-run.jpg".into(),
            accent: "#d7ff64".into(),
            tags: vec!["动物".into(), "冒险".into(), "喜剧".into()],
            episodes: vec![Episode {
                number: 1,
                title: "误入森林".into(),
                duration_seconds: 33,
                video: "assets/videos/forest-run.mp4".into(),
                poster: "assets/posters/forest-run.jpg".into(),
            }],
        },
        Drama {
            id: "llama-drama".into(),
            title: "雪原小羊驼".into(),
            subtitle: "横穿荒原的旅程".into(),
            synopsis: "一只好奇心旺盛的小羊驼踏上雪山旅程，在辽阔天地里遇见意外伙伴，也学会面对自己的胆怯。".into(),
            genre: "冒险动画".into(),
            mood: "治愈".into(),
            duration_minutes: 1,
            poster: "assets/posters/llama-drama.jpg".into(),
            backdrop: "assets/posters/llama-drama.jpg".into(),
            accent: "#ff9c5a".into(),
            tags: vec!["动物".into(), "治愈".into(), "冒险".into()],
            episodes: vec![Episode {
                number: 1,
                title: "雪山相遇".into(),
                duration_seconds: 45,
                video: "assets/videos/llama-drama.mp4".into(),
                poster: "assets/posters/llama-drama.jpg".into(),
            }],
        },
        Drama {
            id: "coffee-run".into(),
            title: "咖啡奇旅".into(),
            subtitle: "把清晨跑成一场电影".into(),
            synopsis: "咖啡、城市和一段不断加速的清晨。看似普通的一天，因为一次奔跑变成了充满想象力的旅程。".into(),
            genre: "都市奇想".into(),
            mood: "轻快".into(),
            duration_minutes: 1,
            poster: "assets/posters/coffee-run.jpg".into(),
            backdrop: "assets/posters/coffee-run.jpg".into(),
            accent: "#6fd6ff".into(),
            tags: vec!["都市".into(), "奇想".into(), "轻快".into()],
            episodes: vec![Episode {
                number: 1,
                title: "晨间奇遇".into(),
                duration_seconds: 40,
                video: "assets/videos/coffee-run.mp4".into(),
                poster: "assets/posters/coffee-run.jpg".into(),
            }],
        },
    ]
}

pub(crate) fn matches(drama: &Drama, query: &str) -> bool {
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
        assert_eq!(view.items.len(), fallback_dramas().len());
    }

    #[test]
    fn unknown_search_falls_back_to_featured_catalog() {
        let view = catalog(Some("不存在的题材"));
        assert_eq!(view.items.len(), fallback_dramas().len());
    }
}
