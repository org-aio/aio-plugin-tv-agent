package site.addzero.tvagent;

import android.content.Context;
import java.io.BufferedReader;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.net.HttpURLConnection;
import java.net.URL;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.util.Locale;
import org.json.JSONArray;
import org.json.JSONObject;

final class PluginRuntime {
    private static final String DEFAULT_AI_ENDPOINT = "https://company-ai.addzero.site/v1";
    private static final String DEFAULT_AI_MODEL = "cn:fast-model";
    private static final String DEFAULT_TVBOX_CONFIG = "https://szyyds.cn/tv/x.json";

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
        JSONArray dramas = fallbackDramas();
        return new JSONObject()
            .put("featured", dramas.getJSONObject(0))
            .put("items", dramas);
    }

    private static JSONObject agent(String message) throws Exception {
        String normalized = message == null ? "" : message.trim();
        if (normalized.isEmpty()) {
            return reply(
                "recommend",
                "你可以直接说片名，例如“我想看斗破苍穹”。",
                new JSONArray().put("我想看斗破苍穹").put("来一部治愈的动物短剧"),
                null
            );
        }

        ParsedIntent parsed = parseIntent(normalized);
        JSONObject remote = searchTvBox(parsed.query);
        if (remote != null) {
            String title = remote.getString("title");
            String text = "play".equals(parsed.intent)
                ? "找到《" + title + "》，共 " + remote.getJSONArray("episodes").length() + " 集。请选择要播放的集数。"
                : "找到《" + title + "》，" + remote.getString("subtitle") + "。";
            return reply(
                parsed.intent,
                text,
                new JSONArray().put("播放" + title).put("还有" + remote.getString("genre") + "这样的影视吗"),
                new JSONObject()
                    .put("drama", remote)
                    .put("episode", remote.getJSONArray("episodes").getJSONObject(0))
            );
        }

        JSONObject selected = matchFallback(normalized);
        if (selected == null) {
            return reply(
                "search",
                "暂时没有匹配的影视资源，可以试试更直接的片名。",
                new JSONArray().put("我想看斗破苍穹").put("播放森林狂奔"),
                null
            );
        }
        String title = selected.getString("title");
        String text = "play".equals(parsed.intent)
            ? "找到适合你的《" + title + "》，共 " + selected.getJSONArray("episodes").length() + " 集。请选择要播放的集数。"
            : "我为你挑选了《" + title + "》，" + selected.getString("subtitle") + "。";
        return reply(
            parsed.intent,
            text,
            new JSONArray().put("播放" + title).put("还有" + selected.getString("genre") + "这样的影视吗"),
            new JSONObject()
                .put("drama", selected)
                .put("episode", selected.getJSONArray("episodes").getJSONObject(0))
        );
    }

    private static ParsedIntent parseIntent(String message) {
        String key = BuildConfig.AI_KEY;
        if (!key.isEmpty()) {
            try {
                JSONObject payload = new JSONObject()
                    .put("model", BuildConfig.AI_MODEL)
                    .put("temperature", 0)
                    .put("response_format", new JSONObject().put("type", "json_object"))
                    .put("messages", new JSONArray()
                        .put(new JSONObject()
                            .put("role", "system")
                            .put("content", "你是电视点播意图解析器。只输出 JSON 对象，格式为 {\"query\":\"用户要搜索的片名或题材\",\"intent\":\"play|search|recommend\"}。去掉“我想看、播放、打开、来一部”等口语，只保留可检索的片名或题材。不要输出 Markdown，不要解释。"))
                        .put(new JSONObject().put("role", "user").put("content", message)));
                JSONObject response = postJson(BuildConfig.AI_ENDPOINT + "/chat/completions", payload, key);
                String content = response.getJSONArray("choices")
                    .getJSONObject(0)
                    .getJSONObject("message")
                    .getString("content");
                JSONObject parsed = new JSONObject(content);
                String query = parsed.optString("query", "").trim();
                if (!query.isEmpty()) {
                    return new ParsedIntent(query, parsed.optString("intent", "play"));
                }
            } catch (Exception ignored) {
                // AI 不可用时回到确定的关键词解析。
            }
        }
        String query = message;
        for (String prefix : new String[] {"我想看", "我要看", "想看", "播放", "看看", "打开", "开始", "来一部", "给我看"}) {
            query = query.replace(prefix, "");
        }
        String intent = message.contains("推荐") ? "recommend" : "play";
        return new ParsedIntent(query.trim(), intent);
    }

    private static JSONObject searchTvBox(String query) {
        if (query.isBlank()) return null;
        try {
            String configUrl = BuildConfig.TVBOX_CONFIG.isEmpty() ? DEFAULT_TVBOX_CONFIG : BuildConfig.TVBOX_CONFIG;
            JSONObject config = getJson(configUrl);
            JSONArray sites = config.optJSONArray("sites");
            if (sites == null) return null;
            for (int index = 0; index < sites.length(); index++) {
                JSONObject site = sites.optJSONObject(index);
                if (site == null || site.optInt("type", -1) != 1 || site.optInt("searchable", 1) == 0) {
                    continue;
                }
                String api = normalizeApi(site.optString("api", ""));
                if (!api.startsWith("https://")) continue;
                JSONObject drama = searchSource(api, query);
                if (drama != null) return drama;
            }
        } catch (Exception ignored) {
        }
        for (String[] source : fallbackSources()) {
            try {
                JSONObject drama = searchSource(source[1], query);
                if (drama != null) return drama;
            } catch (Exception ignored) {
            }
        }
        return null;
    }

    private static JSONObject searchSource(String api, String query) throws Exception {
        JSONObject list = getJson(api + "?ac=list&wd=" + encode(query));
        JSONArray items = list.optJSONArray("list");
        if (items == null || items.length() == 0) return null;
        JSONObject candidate = bestCandidate(items, query);
        if (candidate == null) return null;
        String id = candidate.optString("vod_id", "").trim();
        if (id.isEmpty()) id = String.valueOf(candidate.opt("vod_id"));
        JSONObject detail = getJson(api + "?ac=detail&ids=" + encode(id));
        JSONArray details = detail.optJSONArray("list");
        if (details == null || details.length() == 0) return null;
        return toDrama(details.getJSONObject(0));
    }

    private static JSONObject bestCandidate(JSONArray items, String query) {
        JSONObject best = null;
        int bestScore = 0;
        String normalized = query.toLowerCase(Locale.ROOT);
        for (int index = 0; index < items.length(); index++) {
            JSONObject item = items.optJSONObject(index);
            if (item == null) continue;
            String title = item.optString("vod_name", "").toLowerCase(Locale.ROOT);
            if (title.isEmpty()) continue;
            int score = title.equals(normalized) ? 100 : title.contains(normalized) ? 50 : normalized.contains(title) ? 30 : 0;
            if (score > bestScore) {
                bestScore = score;
                best = item;
            }
        }
        return best;
    }

    private static JSONObject toDrama(JSONObject vod) throws Exception {
        JSONArray episodes = parseEpisodes(vod.optString("vod_play_from", ""), vod.optString("vod_play_url", ""));
        if (episodes.length() == 0) return null;
        String title = clean(vod.optString("vod_name", ""));
        String genre = clean(vod.optString("type_name", vod.optString("vod_class", "影视点播")));
        String poster = vod.optString("vod_pic", "").trim();
        if (!poster.startsWith("https://")) poster = "assets/posters/forest-run.jpg";
        JSONArray tags = new JSONArray();
        for (String tag : genre.split("[,，/]")) {
            if (!tag.trim().isEmpty() && tags.length() < 5) tags.put(tag.trim());
        }
        if (tags.length() == 0) tags.put("影视").put("点播");
        return new JSONObject()
            .put("id", "tvbox-" + vod.optString("vod_id", String.valueOf(title.hashCode())))
            .put("title", title)
            .put("subtitle", clean(vod.optString("vod_remarks", "影视仓资源")))
            .put("synopsis", clean(vod.optString("vod_blurb", vod.optString("vod_content", "来自影视仓的点播资源。"))))
            .put("genre", genre)
            .put("mood", "热播")
            .put("duration_minutes", 0)
            .put("poster", poster)
            .put("backdrop", poster)
            .put("accent", "#d7ff64")
            .put("tags", tags)
            .put("episodes", episodes);
    }

    private static JSONArray parseEpisodes(String from, String urls) throws Exception {
        String[] routes = urls.split("\\$\\$\\$");
        JSONArray best = new JSONArray();
        for (int routeIndex = 0; routeIndex < routes.length; routeIndex++) {
            JSONArray episodes = new JSONArray();
            String[] items = routes[routeIndex].split("#");
            for (int index = 0; index < items.length; index++) {
                int separator = items[index].indexOf('$');
                if (separator < 0) continue;
                String label = items[index].substring(0, separator).trim();
                String url = items[index].substring(separator + 1).trim();
                if (!url.startsWith("https://") || (!url.contains(".m3u8") && !url.contains(".mp4"))) continue;
                episodes.put(new JSONObject()
                    .put("number", index + 1)
                    .put("title", label.isEmpty() ? "第" + (index + 1) + "集" : label)
                    .put("duration_seconds", 0)
                    .put("video", url)
                    .put("poster", ""));
            }
            if (episodes.length() > best.length()) best = episodes;
        }
        return best;
    }

    private static JSONObject matchFallback(String message) throws Exception {
        JSONArray dramas = fallbackDramas();
        String query = message.toLowerCase(Locale.ROOT);
        for (int index = 0; index < dramas.length(); index++) {
            JSONObject drama = dramas.getJSONObject(index);
            if (query.contains(drama.getString("title").toLowerCase(Locale.ROOT))
                || query.contains(drama.getString("mood").toLowerCase(Locale.ROOT))
                || query.contains(drama.getString("genre").toLowerCase(Locale.ROOT))) {
                return drama;
            }
        }
        if (query.contains("治愈") || query.contains("动物")) return dramas.getJSONObject(1);
        if (query.contains("轻松") || query.contains("搞笑") || query.contains("喜剧")) return dramas.getJSONObject(0);
        if (query.contains("都市") || query.contains("奇想") || query.contains("咖啡")) return dramas.getJSONObject(2);
        return null;
    }

    private static JSONObject getJson(String url) throws Exception {
        HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
        connection.setConnectTimeout(8000);
        connection.setReadTimeout(16000);
        connection.setRequestProperty("Accept", "application/json,text/plain,*/*");
        connection.setRequestProperty("User-Agent", "AIO-TV-Agent/0.1 Android");
        try {
            int status = connection.getResponseCode();
            if (status < 200 || status >= 300) throw new IllegalStateException("HTTP " + status);
            return new JSONObject(read(connection.getInputStream()).replace("\uFEFF", "").trim());
        } finally {
            connection.disconnect();
        }
    }

    private static JSONObject postJson(String url, JSONObject payload, String key) throws Exception {
        HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
        connection.setRequestMethod("POST");
        connection.setConnectTimeout(8000);
        connection.setReadTimeout(30000);
        connection.setDoOutput(true);
        connection.setRequestProperty("Content-Type", "application/json");
        connection.setRequestProperty("Authorization", "Bearer " + key);
        byte[] body = payload.toString().getBytes(StandardCharsets.UTF_8);
        connection.setFixedLengthStreamingMode(body.length);
        connection.getOutputStream().write(body);
        try {
            int status = connection.getResponseCode();
            if (status < 200 || status >= 300) throw new IllegalStateException("HTTP " + status);
            return new JSONObject(read(connection.getInputStream()).replace("\uFEFF", "").trim());
        } finally {
            connection.disconnect();
        }
    }

    private static String read(InputStream stream) throws Exception {
        StringBuilder output = new StringBuilder();
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            char[] buffer = new char[4096];
            int count;
            while ((count = reader.read(buffer)) >= 0) {
                output.append(buffer, 0, count);
                if (output.length() > 512000) throw new IllegalStateException("响应超过 512 KB");
            }
        }
        return output.toString();
    }

    private static String encode(String value) throws Exception {
        return URLEncoder.encode(value, StandardCharsets.UTF_8.name());
    }

    private static String normalizeApi(String api) {
        int query = api.indexOf('?');
        return (query >= 0 ? api.substring(0, query) : api).replaceAll("/+$", "");
    }

    private static String clean(String value) {
        return value.replaceAll("<[^>]+>", " ")
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replaceAll("\\s+", " ")
            .trim();
    }

    private static String[][] fallbackSources() {
        return new String[][] {
            {"量子", "https://cj.lziapi.com/api.php/provide/vod"},
            {"极速", "https://jszyapi.com/api.php/provide/vod"},
            {"百度", "https://api.apibdzy.com/api.php/provide/vod"},
            {"暴风", "https://bfzyapi.com/api.php/provide/vod"},
            {"红牛", "https://www.hongniuzy2.com/api.php/provide/vod"},
            {"光速", "https://api.guangsuapi.com/api.php/provide/vod"}
        };
    }

    private static JSONObject reply(String intent, String message, JSONArray suggestions, JSONObject selection)
        throws Exception {
        return new JSONObject()
            .put("intent", intent)
            .put("message", message)
            .put("suggestions", suggestions)
            .put("selection", selection == null ? JSONObject.NULL : selection);
    }

    private static JSONArray fallbackDramas() throws Exception {
        return new JSONArray()
            .put(drama(
                "forest-run",
                "森林狂奔",
                "一只兔子的反击",
                "安静的森林被三个捣蛋鬼打破，主角决定用机智守住自己的家园。",
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
                "一只好奇心旺盛的小羊驼踏上雪山旅程，在辽阔天地里遇见意外伙伴。",
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
                "咖啡、城市和一段不断加速的清晨。",
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

    private record ParsedIntent(String query, String intent) {
    }
}
