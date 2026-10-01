package site.addzero.tvagent;

import android.content.Context;
import android.content.SharedPreferences;
import android.graphics.Color;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URI;
import java.net.URL;
import java.net.URLEncoder;
import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.json.JSONArray;
import org.json.JSONObject;

final class DanmakuClient {
    private static final String SETTINGS = "tv_agent_settings";
    private static final String KEY_DANMAKU_API = "danmaku_api";
    private static final int MAX_BYTES = 4_000_000;
    private static final Pattern XML = Pattern.compile("p=\"([^\"]+)\"[^>]*>([^<]+)<");
    private static final Pattern BRACKET = Pattern.compile("\\[(.*?)\\](.*)");

    private DanmakuClient() {
    }

    static String configuredApi(Context context) {
        return context.getSharedPreferences(SETTINGS, Context.MODE_PRIVATE)
            .getString(KEY_DANMAKU_API, "")
            .trim();
    }

    static void saveApi(Context context, String value) {
        SharedPreferences.Editor editor = context.getSharedPreferences(SETTINGS, Context.MODE_PRIVATE).edit();
        String api = value == null ? "" : value.trim();
        if (api.isEmpty()) {
            editor.remove(KEY_DANMAKU_API);
        } else {
            editor.putString(KEY_DANMAKU_API, api);
        }
        editor.apply();
    }

    static List<DanmakuOverlay.Item> load(Context context, String title, String episode) throws Exception {
        String template = configuredApi(context);
        if (template.isEmpty() || title == null || title.isBlank()) return List.of();
        if (!isSafeApi(template)) throw new IllegalArgumentException("弹幕接口必须为安全的 HTTPS 地址");
        String name = title.trim();
        String episodeName = episode == null ? "" : episode.trim();
        if (template.contains("{name}") || template.contains("{episode}")) {
            String searchUrl = template
                .replace("{name}", encode(name))
                .replace("{episode}", encode(episodeName));
            String search = new String(getBytes(searchUrl), StandardCharsets.UTF_8);
            String dataUrl = resolveDataUrl(search, name, episodeName);
            if (dataUrl.isEmpty()) return List.of();
            return parse(getBytes(dataUrl));
        }
        if (looksLikeDanmakuData(template)) {
            return parse(getBytes(template));
        }
        String search = postText(template, name, episodeName);
        String dataUrl = resolveDataUrl(search, name, episodeName);
        if (dataUrl.isEmpty()) return List.of();
        return parse(getBytes(dataUrl));
    }

    private static boolean looksLikeDanmakuData(String url) {
        String lower = url.toLowerCase();
        return lower.contains(".xml") || lower.contains(".json") || lower.contains("comment");
    }

    private static String resolveDataUrl(String response, String name, String episode) throws Exception {
        String text = response == null ? "" : response.trim();
        if (text.startsWith("[")) {
            JSONArray items = new JSONArray(text);
            if (items.length() == 0) return "";
            JSONObject first = items.optJSONObject(0);
            return first == null ? "" : first.optString("url", "").trim();
        }
        if (text.startsWith("{")) {
            JSONObject object = new JSONObject(text);
            String url = object.optString("url", "").trim();
            if (!url.isEmpty()) return url;
        }
        if (text.startsWith("https://")) {
            return text;
        }
        return "";
    }

    private static List<DanmakuOverlay.Item> parse(byte[] data) throws Exception {
        Charset charset = detectCharset(data);
        String text = new String(data, charset).replace("\uFEFF", "");
        if (text.trim().startsWith("[")) {
            return parseJson(text);
        }
        List<DanmakuOverlay.Item> result = new ArrayList<>();
        Matcher matcher = XML.matcher(text);
        while (matcher.find()) {
            DanmakuOverlay.Item item = item(matcher.group(1), matcher.group(2));
            if (item != null) result.add(item);
        }
        if (result.isEmpty()) {
            String[] lines = text.split("\\R");
            for (String line : lines) {
                Matcher bracket = BRACKET.matcher(line.trim());
                if (bracket.find()) {
                    DanmakuOverlay.Item item = item(bracket.group(1), bracket.group(2));
                    if (item != null) result.add(item);
                }
            }
        }
        return result;
    }

    private static String postText(String url, String name, String episode) throws Exception {
        HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
        connection.setRequestMethod("POST");
        connection.setConnectTimeout(8000);
        connection.setReadTimeout(15000);
        connection.setDoOutput(true);
        connection.setRequestProperty("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8");
        String body = "name=" + encode(name) + "&episode=" + encode(episode);
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        connection.setFixedLengthStreamingMode(bytes.length);
        connection.getOutputStream().write(bytes);
        try {
            int status = connection.getResponseCode();
            if (status < 200 || status >= 300) throw new IllegalStateException("弹幕接口 HTTP " + status);
            return new String(read(connection.getInputStream()), StandardCharsets.UTF_8);
        } finally {
            connection.disconnect();
        }
    }

    private static byte[] getBytes(String url) throws Exception {
        HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
        connection.setConnectTimeout(8000);
        connection.setReadTimeout(15000);
        connection.setRequestProperty("User-Agent", "AIO-TV-Agent/0.1 Android");
        try {
            int status = connection.getResponseCode();
            if (status < 200 || status >= 300) throw new IllegalStateException("弹幕数据 HTTP " + status);
            return read(connection.getInputStream());
        } finally {
            connection.disconnect();
        }
    }

    private static List<DanmakuOverlay.Item> parseJson(String text) {
        List<DanmakuOverlay.Item> result = new ArrayList<>();
        try {
            JSONArray items = new JSONArray(text);
            for (int index = 0; index < items.length(); index++) {
                JSONObject item = items.optJSONObject(index);
                if (item == null) continue;
                long time = item.optLong("time", item.optLong("t", 0L));
                if (time < 100000L) time *= 1000L;
                String content = item.optString("text", item.optString("content", "")).trim();
                if (!content.isEmpty()) result.add(new DanmakuOverlay.Item(time, 1, Color.WHITE, content));
            }
        } catch (Exception ignored) {
        }
        return result;
    }

    private static DanmakuOverlay.Item item(String params, String rawText) {
        try {
            String[] values = params.split(",");
            if (values.length < 4) return null;
            long time = (long) (Float.parseFloat(values[0]) * 1000);
            int type = Integer.parseInt(values[1]);
            int colorValue = Integer.parseInt(values[3]);
            int color = colorValue == 0
                ? Color.WHITE
                : (0xFF000000 | (colorValue & 0xFFFFFF));
            String text = rawText.replace("&quot;", "\"").replace("&gt;", ">").replace("&lt;", "<").replace("&amp;", "&");
            return text.isBlank() ? null : new DanmakuOverlay.Item(time, type, color, text);
        } catch (Exception ignored) {
            return null;
        }
    }


    private static byte[] read(InputStream stream) throws Exception {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        try (InputStream input = stream) {
            byte[] buffer = new byte[8192];
            int count;
            while ((count = input.read(buffer)) >= 0) {
                output.write(buffer, 0, count);
                if (output.size() > MAX_BYTES) throw new IllegalStateException("弹幕数据过大");
            }
        }
        return output.toByteArray();
    }

    private static Charset detectCharset(byte[] data) {
        if (data.length >= 3 && (data[0] & 0xFF) == 0xEF && (data[1] & 0xFF) == 0xBB && (data[2] & 0xFF) == 0xBF) {
            return StandardCharsets.UTF_8;
        }
        String head = new String(data, 0, Math.min(data.length, 512), StandardCharsets.ISO_8859_1);
        Matcher matcher = Pattern.compile("encoding=[\"']([A-Za-z0-9_\\-]+)[\"']").matcher(head);
        if (matcher.find()) {
            try {
                return Charset.forName(matcher.group(1));
            } catch (Exception ignored) {
            }
        }
        return StandardCharsets.UTF_8;
    }

    private static boolean isSafeApi(String value) {
        try {
            URI uri = URI.create(value);
            return "https".equalsIgnoreCase(uri.getScheme())
                && uri.getHost() != null
                && uri.getUserInfo() == null
                && uri.getFragment() == null;
        } catch (Exception ignored) {
            return false;
        }
    }

    private static String encode(String value) throws Exception {
        return URLEncoder.encode(value == null ? "" : value, StandardCharsets.UTF_8.name());
    }

}
