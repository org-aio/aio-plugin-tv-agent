package site.addzero.tvagent;

import android.content.Context;
import java.util.Locale;
import org.json.JSONArray;
import org.json.JSONObject;

final class PluginRuntime {
    private PluginRuntime() {
    }

    static String route(Context context, String request, String tenantId, String userId) throws Exception {
        JSONObject input = new JSONObject(request);
        String method = input.optString("method", "GET");
        String path = input.optString("path", "");
        if (path.startsWith("/api/catalog")) {
            return catalog().toString();
        }
        if ("/api/context".equals(path)) {
            return new JSONObject()
                .put("tenant_id", tenantId)
                .put("user_id", userId)
                .toString();
        }
        if ("POST".equals(method) && "/api/agent".equals(path)) {
            JSONObject body = new JSONObject(input.optString("body", "{}"));
            return agent(body.optString("message", "")).toString();
        }
        throw new IllegalArgumentException("未实现的本地接口: " + method + " " + path);
    }

    private static JSONObject catalog() throws Exception {
        JSONArray dramas = dramas();
        return new JSONObject()
            .put("featured", dramas.getJSONObject(0))
            .put("items", dramas);
    }

    private static JSONObject agent(String message) throws Exception {
        String query = message == null ? "" : message.trim().toLowerCase(Locale.ROOT);
        if (query.isEmpty()) {
            return reply(
                "recommend",
                "你可以告诉我想看轻松、治愈、动物或都市题材，我会直接帮你播放。",
                new JSONArray().put("来一部治愈的动物短剧").put("播放咖啡奇旅"),
                null
            );
        }
        boolean wantsPlay = query.contains("播放")
            || query.contains("看看")
            || query.contains("想看")
            || query.contains("来一部")
            || query.contains("打开")
            || query.contains("开始");
        JSONObject selected = null;
        for (int index = 0; index < dramas().length(); index++) {
            JSONObject drama = dramas().getJSONObject(index);
            if (query.contains(drama.getString("title").toLowerCase(Locale.ROOT))
                || query.contains(drama.getString("mood").toLowerCase(Locale.ROOT))
                || query.contains(drama.getString("genre").toLowerCase(Locale.ROOT))
                || drama.getJSONArray("tags").join(" ").toLowerCase(Locale.ROOT).contains(query)) {
                selected = drama;
                break;
            }
        }
        if (selected == null && (query.contains("治愈") || query.contains("动物"))) {
            selected = dramas().getJSONObject(1);
        }
        if (selected == null && (query.contains("轻松") || query.contains("搞笑") || query.contains("喜剧"))) {
            selected = dramas().getJSONObject(0);
        }
        if (selected == null && (query.contains("都市") || query.contains("奇想") || query.contains("咖啡"))) {
            selected = dramas().getJSONObject(2);
        }
        if (selected == null) {
            return reply(
                "search",
                "暂时没有匹配的短剧，可以试试动物、治愈、冒险或轻松题材。",
                new JSONArray().put("推荐动物短剧").put("播放森林狂奔"),
                null
            );
        }
        String intent = wantsPlay ? "play" : "recommend";
        String title = selected.getString("title");
        String text = wantsPlay
            ? "找到适合你的《" + title + "》，现在开始播放。"
            : "我为你挑选了《" + title + "》，" + selected.getString("subtitle") + "。";
        JSONObject selection = new JSONObject()
            .put("drama", selected)
            .put("episode", selected.getJSONArray("episodes").getJSONObject(0));
        return reply(
            intent,
            text,
            new JSONArray().put("播放" + title).put("还有" + selected.getString("genre") + "这样的短剧吗"),
            selection
        );
    }

    private static JSONObject reply(String intent, String message, JSONArray suggestions, JSONObject selection)
        throws Exception {
        return new JSONObject()
            .put("intent", intent)
            .put("message", message)
            .put("suggestions", suggestions)
            .put("selection", selection == null ? JSONObject.NULL : selection);
    }

    private static JSONArray dramas() throws Exception {
        return new JSONArray()
            .put(drama(
                "forest-run",
                "森林狂奔",
                "一只兔子的反击",
                "安静的森林被三个捣蛋鬼打破，主角决定用机智守住自己的家园。轻松、明快，适合全家一起看。",
                "动画喜剧",
                "轻松",
                "assets/posters/forest-run.jpg",
                "#d7ff64",
                new String[] {"动物", "冒险", "喜剧"},
                episode(1, "误入森林", 33, "assets/videos/forest-run.mp4", "assets/posters/forest-run.jpg")
            ))
            .put(drama(
                "llama-drama",
                "雪原小羊驼",
                "横穿荒原的旅程",
                "一只好奇心旺盛的小羊驼踏上雪山旅程，在辽阔天地里遇见意外伙伴，也学会面对自己的胆怯。",
                "冒险动画",
                "治愈",
                "assets/posters/llama-drama.jpg",
                "#ff9c5a",
                new String[] {"动物", "治愈", "冒险"},
                episode(1, "雪山相遇", 45, "assets/videos/llama-drama.mp4", "assets/posters/llama-drama.jpg")
            ))
            .put(drama(
                "coffee-run",
                "咖啡奇旅",
                "把清晨跑成一场电影",
                "咖啡、城市和一段不断加速的清晨。看似普通的一天，因为一次奔跑变成了充满想象力的旅程。",
                "都市奇想",
                "轻快",
                "assets/posters/coffee-run.jpg",
                "#6fd6ff",
                new String[] {"都市", "奇想", "轻快"},
                episode(1, "晨间奇遇", 40, "assets/videos/coffee-run.mp4", "assets/posters/coffee-run.jpg")
            ));
    }

    private static JSONObject drama(
        String id,
        String title,
        String subtitle,
        String synopsis,
        String genre,
        String mood,
        String poster,
        String accent,
        String[] tags,
        JSONObject episode
    ) throws Exception {
        return new JSONObject()
            .put("id", id)
            .put("title", title)
            .put("subtitle", subtitle)
            .put("synopsis", synopsis)
            .put("genre", genre)
            .put("mood", mood)
            .put("duration_minutes", 1)
            .put("poster", poster)
            .put("backdrop", poster)
            .put("accent", accent)
            .put("tags", new JSONArray(tags))
            .put("episodes", new JSONArray().put(episode));
    }

    private static JSONObject episode(int number, String title, int duration, String video, String poster)
        throws Exception {
        return new JSONObject()
            .put("number", number)
            .put("title", title)
            .put("duration_seconds", duration)
            .put("video", video)
            .put("poster", poster);
    }
}
