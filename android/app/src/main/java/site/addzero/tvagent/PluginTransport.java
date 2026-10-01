package site.addzero.tvagent;

import android.content.Context;
import java.nio.charset.StandardCharsets;
import org.json.JSONObject;

final class PluginTransport {
    private PluginTransport() {
    }

    static String request(Context context, String method, String path, String body) {
        try {
            JSONObject payload = new JSONObject();
            payload.put("method", method);
            payload.put("path", path);
            payload.put("body", body == null ? "" : body);
            byte[] bytes = PluginRuntime.route(context, payload.toString(), "android", "tv-user")
                .getBytes(StandardCharsets.UTF_8);
            return "{\"status\":200,\"body\":\"" + escapeJson(new String(bytes, StandardCharsets.UTF_8)) + "\"}";
        } catch (Exception error) {
            return "{\"status\":500,\"body\":\"{\\\"error\\\":\\\"" + escapeJson(error.getMessage()) + "\\\"}\"}";
        }
    }

    private static String escapeJson(String value) {
        if (value == null) return "";
        StringBuilder output = new StringBuilder(value.length() + 16);
        for (int index = 0; index < value.length(); index++) {
            char character = value.charAt(index);
            switch (character) {
                case '\\' -> output.append("\\\\");
                case '"' -> output.append("\\\"");
                case '\n' -> output.append("\\n");
                case '\r' -> output.append("\\r");
                case '\t' -> output.append("\\t");
                default -> {
                    if (character < 0x20) {
                        output.append(String.format("\\u%04x", (int) character));
                    } else {
                        output.append(character);
                    }
                }
            }
        }
        return output.toString();
    }

}
