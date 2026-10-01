package site.addzero.tvagent;

import android.content.Context;
import android.content.SharedPreferences;
import java.io.BufferedReader;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.net.HttpURLConnection;
import java.net.URL;
import java.net.URLEncoder;
import java.net.URLDecoder;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.CompletionService;
import java.util.concurrent.ExecutorCompletionService;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReferenceArray;
import java.util.concurrent.ConcurrentHashMap;
import org.json.JSONArray;
import org.json.JSONObject;

final class PluginRuntime {
    private static final String DEFAULT_AI_ENDPOINT = "https://company-ai.addzero.site/v1";
    private static final String DEFAULT_AI_MODEL = "auto";
    private static final String[] DEFAULT_TVBOX_CONFIGS = {
        "https://raw.githubusercontent.com/TVboxorg/TVbox/main/dist/official.json",
        "https://raw.githubusercontent.com/wangguo0/tvbox-sub/main/merged.json",
        "https://raw.githubusercontent.com/ZHOUYU86/tvbox/main/b.json",
        "https://raw.githubusercontent.com/hebijunge/tvbox-config/main/tvbox.json",
        "https://raw.githubusercontent.com/haygcao/tvbox-master-aggregator/main/tvbox.json"
    };
    private static final String SETTINGS = "tv_agent_settings";
    private static final String KEY_ENDPOINT = "model_endpoint";
    private static final String KEY_MODEL = "model";
    private static final String KEY_TVBOX_CONFIGS = "tvbox_configs";
    private static final String KEY_DANMAKU_API = "danmaku_api";
    private static final int MAX_CONFIG_INDEX_ENTRIES = 12;
    private static final int MAX_SEARCH_SOURCES = 24;
    private static final int MAX_API_RESPONSE_CHARS = 512_000;
    private static final int MAX_LIVE_RESPONSE_CHARS = 2_000_000;
    private static final int MAX_CONFIG_RESPONSE_CHARS = 4_000_000;
    private static final long CONFIG_SOURCE_TIMEOUT_SECONDS = 10;
    private static final long SOURCE_SEARCH_TIMEOUT_SECONDS = 8;
    private static final long CACHE_TTL_MILLIS = 10 * 60 * 1000L;
    private static final long SOURCE_CACHE_TTL_MILLIS = 30 * 60 * 1000L;
    private static final ConcurrentHashMap<String, CacheValue<JSONObject>> CACHE = new ConcurrentHashMap<>();
    private static final ConcurrentHashMap<String, CacheValue<List<String[]>>> SOURCE_CACHE = new ConcurrentHashMap<>();

    private PluginRuntime() {
    }

    static String route(Context context, String request, String tenantId, String userId) throws Exception {
        JSONObject input = new JSONObject(request);
        String method = input.optString("method", "GET");
        String path = input.optString("path", "");
        if (path.startsWith("/api/catalog")) {
            return catalog().toString();
        }
        if (path.startsWith("/api/browse")) {
            return browse(context, parseQuery(path)).toString();
        }
        if ("/api/context".equals(path)) {
            return new JSONObject()
                .put("tenant_id", tenantId)
                .put("user_id", userId)
                .toString();
        }
        if ("GET".equals(method) && "/api/settings".equals(path)) {
            return settings(context).toString();
        }
        if ("POST".equals(method) && "/api/settings".equals(path)) {
            return saveSettings(context, new JSONObject(input.optString("body", "{}"))).toString();
        }
        if ("POST".equals(method) && "/api/models".equals(path)) {
            JSONObject body = new JSONObject(input.optString("body", "{}"));
            return listModels(context, body).toString();
        }
        if ("POST".equals(method) && "/api/models/test".equals(path)) {
            JSONObject body = new JSONObject(input.optString("body", "{}"));
            return testModel(context, body).toString();
        }
        if ("POST".equals(method) && "/api/agent".equals(path)) {
            JSONObject body = new JSONObject(input.optString("body", "{}"));
            return agent(context, body.optString("message", "")).toString();
        }
        throw new IllegalArgumentException("未实现的本地接口: " + method + " " + path);
    }

    private static JSONObject browse(Context context, QueryParams query) {
        String category = query.value("category", "all").trim();
        int page = Math.max(1, query.integer("page", 1));
        int pageSize = Math.max(1, Math.min(40, query.integer("page_size", 24)));
        String key = "browse:"
            + String.join("\n", configuredTvboxUrls(context))
            + ":" + category + ":" + page + ":" + pageSize;
        JSONObject cached = cached(key);
        if (cached != null) return cached;
        JSONObject expired = cached(key, true);
        if (expired != null) {
            refreshBrowse(context, key, category, page, pageSize);
            return expired;
        }

        JSONObject result;
        try {
            result = "live".equals(category)
                ? browseLive(context, page, pageSize)
                : browseVod(context, category, page, pageSize);
            store(key, result);
        } catch (Exception error) {
            JSONObject stale = cached(key, true);
            if (stale != null) return stale;
            throw new IllegalStateException(error.getMessage(), error);
        }
        return result;
    }

    private static void refreshBrowse(Context context, String key, String category, int page, int pageSize) {
        Thread refresh = new Thread(() -> {
            try {
                JSONObject result = "live".equals(category)
                    ? browseLive(context, page, pageSize)
                    : browseVod(context, category, page, pageSize);
                store(key, result);
            } catch (Exception ignored) {
                // 后台刷新失败时保留已有缓存。
            }
        }, "tv-agent-cache-refresh");
        refresh.setDaemon(true);
        refresh.start();
    }

    private static JSONObject catalog() throws Exception {
        JSONArray dramas = fallbackDramas();
        JSONArray sections = new JSONArray()
            .put(section("featured", "热播精选", dramas))
            .put(section("movie", "电影", filterSection(dramas, "movie")))
            .put(section("anime", "动漫", filterSection(dramas, "anime")))
            .put(section("series", "电视剧", filterSection(dramas, "series")))
            .put(section("variety", "综艺", filterSection(dramas, "variety")))
            .put(section("short", "短剧速看", filterSection(dramas, "short")));
        return new JSONObject()
            .put("featured", dramas.getJSONObject(0))
            .put("sections", nonEmptySections(sections));
    }

    private static JSONObject section(String id, String title, JSONArray items) throws Exception {
        return new JSONObject().put("id", id).put("title", title).put("items", items);
    }

    private static JSONArray filterSection(JSONArray dramas, String type) throws Exception {
        JSONArray result = new JSONArray();
        for (int index = 0; index < dramas.length(); index++) {
            JSONObject drama = dramas.getJSONObject(index);
            if (type.equals(drama.optString("content_type", ""))) result.put(drama);
        }
        return result;
    }

    private static JSONArray nonEmptySections(JSONArray sections) throws Exception {
        JSONArray result = new JSONArray();
        for (int index = 0; index < sections.length(); index++) {
            JSONObject section = sections.getJSONObject(index);
            if (section.getJSONArray("items").length() > 0) result.put(section);
        }
        return result;
    }

    private static JSONObject agent(Context context, String message) throws Exception {
        String normalized = message == null ? "" : message.trim();
        if (normalized.isEmpty()) {
            return reply(
                "recommend",
                "你可以直接说片名，例如“我想看斗破苍穹”。",
                new JSONArray().put("我想看斗破苍穹").put("来一部治愈的动物短剧"),
                null
            );
        }

        ParsedIntent parsed = parseIntent(context, normalized);
        JSONObject remote = searchTvBox(context, parsed.query);
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

    private static ParsedIntent parseIntent(Context context, String message) {
        RuntimeSettings settings = runtimeSettings(context);
        String key = settings.secret;
        if (!key.isEmpty()) {
            try {
                JSONObject payload = new JSONObject()
                    .put("model", settings.model)
                    .put("temperature", 0)
                    .put("response_format", new JSONObject().put("type", "json_object"))
                    .put("messages", new JSONArray()
                        .put(new JSONObject()
                            .put("role", "system")
                            .put("content", "你是电视点播意图解析器。只输出 JSON 对象，格式为 {\"query\":\"用户要搜索的片名或题材\",\"intent\":\"play|search|recommend\"}。去掉“我想看、播放、打开、来一部”等口语，只保留可检索的片名或题材。不要输出 Markdown，不要解释。"))
                        .put(new JSONObject().put("role", "user").put("content", message)));
                JSONObject response = postJson(settings.endpoint + "/chat/completions", payload, key);
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

    private static JSONObject settings(Context context) throws Exception {
        RuntimeSettings settings = runtimeSettings(context);
        return new JSONObject()
            .put("model_endpoint", settings.endpoint)
            .put("model", settings.model)
            .put("has_secret", !settings.secret.isEmpty())
            .put("tvbox_configs", tvboxConfigs(context))
            .put("danmaku_api", danmakuApi(context));
    }

    private static JSONObject saveSettings(Context context, JSONObject body) throws Exception {
        RuntimeSettings current = runtimeSettings(context);
        String endpoint = normalizeEndpoint(body.optString("model_endpoint", current.endpoint));
        String model = body.optString("model", current.model).trim();
        if (!isSafeEndpoint(endpoint) || model.isEmpty()) {
            throw new IllegalArgumentException("模型地址必须为 HTTPS，且模型名不能为空");
        }
        String configured = body.optJSONArray("tvbox_configs") == null
            ? tvboxConfigs(context).toString()
            : body.optJSONArray("tvbox_configs").toString();
        String danmakuApi = body.has("danmaku_api")
            ? validateDanmakuApi(body.optString("danmaku_api", ""))
            : danmakuApi(context);
        JSONArray configs = new JSONArray(configured);
        if (configs.length() == 0 || configs.length() > 16) {
            throw new IllegalArgumentException("TVBox 配置地址数量必须为 1 到 16 个");
        }
        StringBuilder configValues = new StringBuilder();
        for (int index = 0; index < configs.length(); index++) {
            String value = configs.optString(index, "").trim();
            if (!isSafeConfigUrl(value)) {
                throw new IllegalArgumentException("TVBox 配置地址必须是无凭据的 HTTPS 地址");
            }
            if (configValues.length() > 0) configValues.append(',');
            configValues.append(value);
        }
        SharedPreferences.Editor editor = context.getSharedPreferences(SETTINGS, Context.MODE_PRIVATE).edit()
            .putString(KEY_ENDPOINT, endpoint)
            .putString(KEY_MODEL, model)
            .putString(KEY_TVBOX_CONFIGS, configValues.toString())
            .putString(KEY_DANMAKU_API, danmakuApi);
        if (body.has("secret")) {
            SecretStore.save(context, body.optString("secret", "").trim());
        } else if (!endpoint.equals(current.endpoint)) {
            SecretStore.clear(context);
        }
        editor.apply();
        return settings(context);
    }

    private static JSONArray listModels(Context context, JSONObject body) throws Exception {
        RuntimeSettings saved = runtimeSettings(context);
        String endpoint = normalizeEndpoint(body.optString("model_endpoint", saved.endpoint));
        if (!isSafeEndpoint(endpoint)) throw new IllegalArgumentException("模型地址必须为 HTTPS");
        String secret = body.optString("secret", "").trim();
        if (secret.isEmpty() && endpoint.equals(saved.endpoint)) secret = saved.secret;
        JSONObject response = getJson(endpoint + "/models", secret);
        JSONArray data = response.optJSONArray("data");
        List<String> models = new ArrayList<>();
        if (data != null) {
            for (int index = 0; index < data.length(); index++) {
                JSONObject item = data.optJSONObject(index);
                String id = item == null ? "" : item.optString("id", "").trim();
                if (!id.isEmpty() && id.length() <= 160 && !models.contains(id)) models.add(id);
            }
        }
        Collections.sort(models);
        return new JSONArray(models);
    }

    private static JSONObject testModel(Context context, JSONObject body) throws Exception {
        RuntimeSettings saved = runtimeSettings(context);
        String endpoint = normalizeEndpoint(body.optString("model_endpoint", saved.endpoint));
        String model = body.optString("model", saved.model).trim();
        String secret = body.optString("secret", "").trim();
        if (secret.isEmpty() && endpoint.equals(saved.endpoint)) secret = saved.secret;
        if (!isSafeEndpoint(endpoint)) throw new IllegalArgumentException("模型地址必须为 HTTPS");
        if (model.isEmpty()) throw new IllegalArgumentException("模型名不能为空");
        if (secret.isEmpty()) throw new IllegalArgumentException("未配置 AI Key");
        JSONObject payload = new JSONObject()
            .put("model", model)
            .put("stream", false)
            .put("max_tokens", 8)
            .put("messages", new JSONArray().put(new JSONObject().put("role", "user").put("content", "只回复 OK")));
        JSONObject response = postJson(endpoint + "/chat/completions", payload, secret);
        JSONArray choices = response.optJSONArray("choices");
        if (choices == null || choices.length() == 0) throw new IllegalStateException("模型没有返回推理结果");
        return new JSONObject().put("ok", true).put("model_endpoint", endpoint).put("model", model);
    }

    private static RuntimeSettings runtimeSettings(Context context) {
        SharedPreferences preferences = context.getSharedPreferences(SETTINGS, Context.MODE_PRIVATE);
        return new RuntimeSettings(
            normalizeEndpoint(preferences.getString(KEY_ENDPOINT, defaultEndpoint())),
            preferences.getString(KEY_MODEL, defaultModel()).trim(),
            SecretStore.load(context)
        );
    }

    private static JSONArray tvboxConfigs(Context context) {
        String configured = context.getSharedPreferences(SETTINGS, Context.MODE_PRIVATE)
            .getString(KEY_TVBOX_CONFIGS, "");
        JSONArray result = new JSONArray();
        if (configured != null && !configured.isBlank()) {
            for (String value : configured.split(",")) {
                String url = value.trim();
                if (url.startsWith("https://")) result.put(url);
            }
        }
        if (result.length() == 0) {
            for (String url : DEFAULT_TVBOX_CONFIGS) result.put(url);
        }
        return result;
    }

    private static String danmakuApi(Context context) {
        return context.getSharedPreferences(SETTINGS, Context.MODE_PRIVATE)
            .getString(KEY_DANMAKU_API, "")
            .trim();
    }

    private static String validateDanmakuApi(String value) {
        String api = value == null ? "" : value.trim();
        if (api.isEmpty()) {
            return "";
        }
        try {
            URI uri = URI.create(api);
            if (!"https".equalsIgnoreCase(uri.getScheme())
                || uri.getHost() == null
                || uri.getUserInfo() != null
                || uri.getFragment() != null) {
                throw new IllegalArgumentException("弹幕接口必须是无凭据的 HTTPS 地址");
            }
            return api;
        } catch (IllegalArgumentException error) {
            throw new IllegalArgumentException("弹幕接口必须是无凭据的 HTTPS 地址");
        }
    }

    private static String defaultEndpoint() {
        return BuildConfig.AI_ENDPOINT.isEmpty() ? DEFAULT_AI_ENDPOINT : BuildConfig.AI_ENDPOINT;
    }

    private static String defaultModel() {
        return BuildConfig.AI_MODEL.isEmpty() ? DEFAULT_AI_MODEL : BuildConfig.AI_MODEL;
    }

    private static String normalizeEndpoint(String value) {
        return value == null ? "" : value.trim().replaceAll("/+$", "");
    }

    private static boolean isSafeEndpoint(String value) {
        try {
            URI uri = URI.create(value);
            return "https".equalsIgnoreCase(uri.getScheme())
                && uri.getHost() != null
                && uri.getUserInfo() == null
                && uri.getQuery() == null
                && uri.getFragment() == null;
        } catch (IllegalArgumentException ignored) {
            return false;
        }
    }

    private static JSONObject searchTvBox(Context context, String query) {
        if (query.isBlank()) return null;
        List<String[]> sources = collectSources(context);
        if (sources.isEmpty()) return null;

        int workers = Math.min(8, sources.size());
        ExecutorService executor = Executors.newFixedThreadPool(workers);
        CompletionService<JSONObject> completion = new ExecutorCompletionService<>(executor);
        List<Future<JSONObject>> tasks = new ArrayList<>();
        AtomicReferenceArray<JSONObject> results =
            new AtomicReferenceArray<>(Math.min(sources.size(), MAX_SEARCH_SOURCES));
        try {
            for (int index = 0; index < results.length(); index++) {
                final String[] source = sources.get(index);
                final int sourceIndex = index;
                tasks.add(completion.submit(() -> {
                    JSONObject drama = null;
                    try {
                        drama = searchSource(source[0], query);
                    } catch (Exception ignored) {
                    }
                    results.set(sourceIndex, drama);
                    return drama;
                }));
            }
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(SOURCE_SEARCH_TIMEOUT_SECONDS);
            for (int completed = 0; completed < tasks.size(); completed++) {
                long remaining = deadline - System.nanoTime();
                if (remaining <= 0) break;
                Future<JSONObject> future = completion.poll(remaining, TimeUnit.NANOSECONDS);
                if (future == null) break;
                future.get();
            }
            for (int index = 0; index < results.length(); index++) {
                JSONObject drama = results.get(index);
                if (drama != null) return drama;
            }
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
        } catch (Exception ignored) {
        } finally {
            for (Future<JSONObject> task : tasks) task.cancel(true);
            executor.shutdownNow();
        }
        return null;
    }

    private static JSONObject browseVod(Context context, String category, int page, int pageSize) {
        List<String[]> sources = collectSources(context);
        if (sources.isEmpty()) return emptyBrowse(category, page);
        int workers = Math.min(8, sources.size());
        ExecutorService executor = Executors.newFixedThreadPool(workers);
        CompletionService<List<JSONObject>> completion = new ExecutorCompletionService<>(executor);
        List<Future<List<JSONObject>>> tasks = new ArrayList<>();
        List<JSONObject> items = new ArrayList<>();
        java.util.Set<String> seen = new java.util.HashSet<>();
        boolean hasMore = false;
        int total = 0;
        try {
            for (int index = 0; index < Math.min(sources.size(), 12); index++) {
                String[] source = sources.get(index);
                tasks.add(completion.submit(() -> browseSource(source[0], category, page)));
            }
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(12);
            for (int completed = 0; completed < tasks.size(); completed++) {
                long remaining = deadline - System.nanoTime();
                if (remaining <= 0) break;
                Future<List<JSONObject>> future = completion.poll(remaining, TimeUnit.NANOSECONDS);
                if (future == null) break;
                try {
                    List<JSONObject> pageItems = future.get();
                    if (!pageItems.isEmpty()) hasMore = true;
                    for (JSONObject drama : pageItems) {
                        String type = drama.optString("content_type", "");
                        if (!category.isEmpty() && !"all".equals(category) && !category.equals(type)) continue;
                        String dedupe = drama.optString("title", "") + "|" + type;
                        if (seen.add(dedupe)) items.add(drama);
                    }
                    total += pageItems.size();
                } catch (Exception ignored) {
                }
            }
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
        } finally {
            for (Future<List<JSONObject>> task : tasks) task.cancel(true);
            executor.shutdownNow();
        }
        if (items.size() > pageSize) items = new ArrayList<>(items.subList(0, pageSize));
        return browseResult(items, page, pageSize, hasMore, total, category);
    }

    private static List<JSONObject> browseSource(String api, String category, int page) {
        try {
            String classId = "all".equals(category) || category.isEmpty()
                ? null
                : browseClassId(api, category);
            StringBuilder url = new StringBuilder(api)
                .append("?ac=detail&pg=")
                .append(page);
            if (classId != null && !classId.isBlank()) url.append("&t=").append(encode(classId));
            JSONArray list = getJson(url.toString()).optJSONArray("list");
            List<JSONObject> items = new ArrayList<>();
            if (list == null) return items;
            for (int index = 0; index < list.length(); index++) {
                JSONObject vod = list.optJSONObject(index);
                if (vod == null) continue;
                JSONObject drama = toDrama(vod);
                if (drama != null) items.add(drama);
            }
            return items;
        } catch (Exception ignored) {
            return Collections.emptyList();
        }
    }

    private static String browseClassId(String api, String category) {
        try {
            JSONArray classes = getJson(api + "?ac=list").optJSONArray("class");
            if (classes == null) return null;
            String[] names = switch (category) {
                case "movie" -> new String[] {"电影", "电影片"};
                case "anime" -> new String[] {"动漫", "动漫片", "动画", "番剧"};
                case "series" -> new String[] {"电视剧", "连续剧"};
                case "variety" -> new String[] {"综艺", "综艺片", "真人秀", "脱口秀"};
                case "short" -> new String[] {"短剧", "短剧大全", "爽文短剧", "漫剧"};
                default -> new String[0];
            };
            for (int index = 0; index < classes.length(); index++) {
                JSONObject item = classes.optJSONObject(index);
                if (item == null || !"0".equals(String.valueOf(item.opt("type_pid")))) continue;
                String typeName = item.optString("type_name", "").trim();
                for (String name : names) {
                    if (typeName.equals(name)) return String.valueOf(item.opt("type_id"));
                }
            }
            return null;
        } catch (Exception ignored) {
            return null;
        }
    }

    private static JSONObject browseLive(Context context, int page, int pageSize) {
        List<JSONObject> channels = new ArrayList<>();
        for (String[] playlist : new String[][] {
            {"虎牙一起看", "https://sub.ottiptv.cc/huyayqk.m3u"},
            {"综合直播", "https://down.nigx.cn/raw.githubusercontent.com/Kimentanm/aptv/master/m3u/iptv.m3u"},
            {"综合电视", "https://raw.liucn.cc/box/libs/tv/tvlive.txt"},
            {"IPTV 直播", "https://z.szyyds.cn/iptv"}
        }) {
            try {
                channels.addAll(parseLivePlaylist(getText(playlist[1]), playlist[0]));
            } catch (Exception ignored) {
            }
        }
        int start = Math.max(0, (page - 1) * pageSize);
        int end = Math.min(channels.size(), start + pageSize);
        List<JSONObject> pageItems = start >= channels.size()
            ? Collections.emptyList()
            : new ArrayList<>(channels.subList(start, end));
        return browseResult(pageItems, page, pageSize, end < channels.size(), channels.size(), "live");
    }

    private static List<JSONObject> parseLivePlaylist(String text, String playlistName) throws Exception {
        List<JSONObject> result = new ArrayList<>();
        String pending = "";
        for (String rawLine : text.split("\\R")) {
            String line = rawLine.trim();
            if (line.isEmpty() || line.startsWith("#") && !line.startsWith("#EXTINF")) continue;
            if (line.startsWith("#EXTINF")) {
                int separator = line.indexOf(',');
                pending = separator >= 0 ? clean(line.substring(separator + 1)) : "";
                continue;
            }
            if (!line.startsWith("http://") && !line.startsWith("https://")) continue;
            String title = pending.isEmpty() ? playlistName : pending;
            pending = "";
            if (title.contains("更新时间") || line.contains("127.0.0.1") || line.contains("localhost")) continue;
            result.add(new JSONObject()
                .put("id", "live-" + Math.abs((title + "|" + line).hashCode()))
                .put("title", title)
                .put("subtitle", playlistName + " · 直播")
                .put("synopsis", "来自" + playlistName + "的直播频道。")
                .put("genre", "直播")
                .put("content_type", "live")
                .put("mood", "直播中")
                .put("duration_minutes", 0)
                .put("poster", "assets/posters/forest-run.jpg")
                .put("backdrop", "assets/posters/forest-run.jpg")
                .put("accent", "#ff7a59")
                .put("tags", new JSONArray().put("直播").put("频道"))
                .put("episodes", new JSONArray().put(new JSONObject()
                    .put("number", 1)
                    .put("title", "直播")
                    .put("duration_seconds", 0)
                    .put("video", line)
                    .put("poster", ""))));
        }
        return result;
    }

    private static JSONObject browseResult(
        List<JSONObject> items,
        int page,
        int pageSize,
        boolean hasMore,
        int total,
        String category
    ) {
        try {
            return new JSONObject()
                .put("items", new JSONArray(items))
                .put("page", page)
                .put("next_page", hasMore ? page + 1 : 0)
                .put("has_more", hasMore)
                .put("total", total)
                .put("category", category);
        } catch (Exception error) {
            throw new IllegalStateException("构建片库结果失败", error);
        }
    }

    private static JSONObject emptyBrowse(String category, int page) {
        return browseResult(Collections.emptyList(), page, 24, false, 0, category);
    }

    private static List<String[]> collectSources(Context context) {
        List<String> configUrls = configuredTvboxUrls(context);
        String cacheKey = "sources:" + String.join("\n", configUrls);
        List<String[]> cached = cachedSources(cacheKey, SOURCE_CACHE_TTL_MILLIS);
        if (cached != null) return cached;

        List<String[]> sources = new ArrayList<>();
        for (String[] source : fallbackSources()) addSource(sources, source[1], source[0]);

        if (configUrls.isEmpty()) {
            storeSources(cacheKey, sources);
            return sources;
        }
        int workers = Math.min(6, configUrls.size());
        ExecutorService executor = Executors.newFixedThreadPool(workers);
        CompletionService<List<String[]>> completion = new ExecutorCompletionService<>(executor);
        List<Future<List<String[]>>> tasks = new ArrayList<>();
        try {
            for (String configUrl : configUrls) {
                tasks.add(completion.submit(() -> {
                    List<String[]> configured = new ArrayList<>();
                    try {
                        collectSources(getConfigJson(configUrl), configured);
                    } catch (Exception ignored) {
                    }
                    return configured;
                }));
            }
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(CONFIG_SOURCE_TIMEOUT_SECONDS);
            for (int completed = 0; completed < tasks.size(); completed++) {
                long remaining = deadline - System.nanoTime();
                if (remaining <= 0) break;
                Future<List<String[]>> future = completion.poll(remaining, TimeUnit.NANOSECONDS);
                if (future == null) break;
                for (String[] source : future.get()) addSource(sources, source[0], source[1]);
            }
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
        } catch (Exception ignored) {
        } finally {
            for (Future<List<String[]>> task : tasks) task.cancel(true);
            executor.shutdownNow();
        }
        if (sources.size() == fallbackSources().length) {
            List<String[]> stale = cachedSources(cacheKey, Long.MAX_VALUE);
            if (stale != null) return stale;
        }
        storeSources(cacheKey, sources);
        return sources;
    }

    private static List<String[]> copySources(List<String[]> sources) {
        List<String[]> copy = new ArrayList<>(sources.size());
        for (String[] source : sources) copy.add(new String[] {source[0], source[1]});
        return copy;
    }

    private static List<String> configuredTvboxUrls(Context context) {
        List<String> urls = new ArrayList<>();
        JSONArray configured = tvboxConfigs(context);
        for (int index = 0; index < configured.length(); index++) {
            String url = configured.optString(index, "").trim();
            if (isSafeConfigUrl(url)) urls.add(url);
        }
        return urls;
    }

    private static void collectSources(JSONObject config, List<String[]> sources) throws Exception {
        JSONArray indexes = config.optJSONArray("urls");
        if (indexes != null) {
            for (int index = 0; index < Math.min(indexes.length(), 12); index++) {
                JSONObject entry = indexes.optJSONObject(index);
                if (entry == null) continue;
                String url = entry.optString("url", "").trim();
                if (!url.startsWith("https://")) continue;
                try {
                    collectSources(getConfigJson(url), sources);
                } catch (Exception ignored) {
                }
            }
            return;
        }
        JSONArray sites = config.optJSONArray("sites");
        if (sites == null) return;
        for (int index = 0; index < sites.length(); index++) {
            JSONObject site = sites.optJSONObject(index);
            if (site == null || site.optInt("type", -1) != 1 || site.optInt("searchable", 1) == 0) {
                continue;
            }
            addSource(sources, normalizeApi(site.optString("api", "")), site.optString("name", site.optString("key", "影视源")));
        }
    }

    private static void addSource(List<String[]> sources, String api, String name) {
        if (!api.startsWith("https://")) return;
        for (String[] source : sources) if (source[0].equals(api)) return;
        sources.add(new String[] {api, name});
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
        String contentType = contentType(genre, tags);
        return new JSONObject()
            .put("id", "tvbox-" + vod.optString("vod_id", String.valueOf(title.hashCode())))
            .put("title", title)
            .put("subtitle", clean(vod.optString("vod_remarks", "影视仓资源")))
            .put("synopsis", clean(vod.optString("vod_blurb", vod.optString("vod_content", "来自影视仓的点播资源。"))))
            .put("genre", genre)
            .put("content_type", contentType)
            .put("mood", "热播")
            .put("duration_minutes", 0)
            .put("poster", poster)
            .put("backdrop", poster)
            .put("accent", "#d7ff64")
            .put("tags", tags)
            .put("episodes", episodes);
    }

    private static String contentType(String genre, JSONArray tags) {
        StringBuilder haystack = new StringBuilder(genre == null ? "" : genre);
        for (int index = 0; index < tags.length(); index++) {
            haystack.append(' ').append(tags.optString(index, ""));
        }
        String value = haystack.toString().toLowerCase(Locale.ROOT);
        if (containsAny(value, "动漫", "动画", "国漫", "日漫", "番剧")) return "anime";
        if (value.contains("电影")) return "movie";
        if (containsAny(value, "综艺", "真人秀", "脱口秀")) return "variety";
        if (value.contains("短剧")) return "short";
        return "series";
    }

    private static boolean containsAny(String value, String... keywords) {
        for (String keyword : keywords) {
            if (value.contains(keyword)) return true;
        }
        return false;
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
                if ((!url.startsWith("https://") && !url.startsWith("http://"))
                    || (!url.contains(".m3u8") && !url.contains(".mp4"))) {
                    continue;
                }
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
        return getJson(url, null, MAX_API_RESPONSE_CHARS);
    }

    private static JSONObject getJson(String url, String key) throws Exception {
        return getJson(url, key, MAX_API_RESPONSE_CHARS);
    }

    private static JSONObject getConfigJson(String url) throws Exception {
        return getJson(url, null, MAX_CONFIG_RESPONSE_CHARS);
    }

    private static String getText(String url) throws Exception {
        HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
        connection.setConnectTimeout(8000);
        connection.setReadTimeout(16000);
        connection.setRequestProperty("Accept", "text/plain,application/x-mpegURL,*/*");
        connection.setRequestProperty("User-Agent", "AIO-TV-Agent/0.1 Android");
        try {
            int status = connection.getResponseCode();
            if (status < 200 || status >= 300) throw new IllegalStateException("HTTP " + status);
            return read(connection.getInputStream(), MAX_LIVE_RESPONSE_CHARS).replace("\uFEFF", "").trim();
        } finally {
            connection.disconnect();
        }
    }

    private static JSONObject getJson(String url, String key, int maxResponseChars) throws Exception {
        HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
        connection.setConnectTimeout(8000);
        connection.setReadTimeout(16000);
        connection.setRequestProperty("Accept", "application/json,text/plain,*/*");
        connection.setRequestProperty("User-Agent", "AIO-TV-Agent/0.1 Android");
        if (key != null && !key.isEmpty()) connection.setRequestProperty("Authorization", "Bearer " + key);
        try {
            int status = connection.getResponseCode();
            if (status < 200 || status >= 300) throw new IllegalStateException("HTTP " + status);
            return new JSONObject(read(connection.getInputStream(), maxResponseChars).replace("\uFEFF", "").trim());
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
            return new JSONObject(read(connection.getInputStream(), MAX_API_RESPONSE_CHARS).replace("\uFEFF", "").trim());
        } finally {
            connection.disconnect();
        }
    }

    private static String read(InputStream stream) throws Exception {
        return read(stream, MAX_API_RESPONSE_CHARS);
    }

    private static String read(InputStream stream, int maxResponseChars) throws Exception {
        StringBuilder output = new StringBuilder();
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            char[] buffer = new char[4096];
            int count;
            while ((count = reader.read(buffer)) >= 0) {
                output.append(buffer, 0, count);
                if (output.length() > maxResponseChars) {
                    throw new IllegalStateException("响应超过 " + maxResponseChars + " 字符");
                }
            }
        }
        return output.toString();
    }

    private static String encode(String value) throws Exception {
        return URLEncoder.encode(value, StandardCharsets.UTF_8.name());
    }

    private static QueryParams parseQuery(String path) {
        Map<String, String> values = new HashMap<>();
        try {
            String query = URI.create(path).getRawQuery();
            if (query == null || query.isBlank()) return new QueryParams(values);
            for (String pair : query.split("&")) {
                if (pair.isBlank()) continue;
                String[] parts = pair.split("=", 2);
                String key = URLDecoder.decode(parts[0], StandardCharsets.UTF_8.name());
                String value = parts.length == 2 ? URLDecoder.decode(parts[1], StandardCharsets.UTF_8.name()) : "";
                values.put(key, value);
            }
        } catch (Exception ignored) {
            return new QueryParams(Collections.emptyMap());
        }
        return new QueryParams(values);
    }

    private static JSONObject cached(String key) {
        return cached(key, false);
    }

    private static JSONObject cached(String key, boolean allowStale) {
        CacheValue<JSONObject> entry = CACHE.get(key);
        if (entry == null) return null;
        if (allowStale || System.currentTimeMillis() - entry.storedAt <= CACHE_TTL_MILLIS) {
            return entry.value;
        }
        return null;
    }

    private static void store(String key, JSONObject value) {
        CACHE.put(key, new CacheValue<>(value, System.currentTimeMillis()));
    }

    private static List<String[]> cachedSources(String key, long ttlMillis) {
        CacheValue<List<String[]>> entry = SOURCE_CACHE.get(key);
        if (entry == null) return null;
        if (ttlMillis == Long.MAX_VALUE || System.currentTimeMillis() - entry.storedAt <= ttlMillis) {
            return copySources(entry.value);
        }
        return null;
    }

    private static void storeSources(String key, List<String[]> value) {
        SOURCE_CACHE.put(key, new CacheValue<>(copySources(value), System.currentTimeMillis()));
    }

    private static boolean isSafeConfigUrl(String value) {
        try {
            URI uri = URI.create(value);
            return "https".equalsIgnoreCase(uri.getScheme())
                && uri.getHost() != null
                && uri.getUserInfo() == null
                && uri.getQuery() == null
                && uri.getFragment() == null;
        } catch (IllegalArgumentException ignored) {
            return false;
        }
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
            {"360", "https://360zy.com/api.php/provide/vod"},
            {"电影天堂", "https://caiji.dyttzyapi.com/api.php/provide/vod"},
            {"非凡", "https://ffzy.tv/api.php/provide/vod"},
            {"豆瓣", "https://cdn.dzzyapi.com/api.php/provide/vod"},
            {"百度", "https://api.apibdzy.com/api.php/provide/vod"},
            {"光速", "https://api.guangsuapi.com/api.php/provide/vod"},
            {"暴风", "https://bfzyapi.com/api.php/provide/vod"},
            {"红牛", "https://www.hongniuzy2.com/api.php/provide/vod"}
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
                "anime",
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
                "anime",
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
                "都市电影",
                "movie",
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
        String contentType,
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
            .put("content_type", contentType)
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

    private record RuntimeSettings(String endpoint, String model, String secret) {
    }

    private record CacheValue<T>(T value, long storedAt) {
    }

    private record QueryParams(Map<String, String> values) {
        private String value(String key, String fallback) {
            return values.getOrDefault(key, fallback);
        }

        private int integer(String key, int fallback) {
            try {
                return Integer.parseInt(value(key, String.valueOf(fallback)));
            } catch (NumberFormatException ignored) {
                return fallback;
            }
        }
    }
}
