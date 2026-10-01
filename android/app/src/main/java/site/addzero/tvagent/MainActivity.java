package site.addzero.tvagent;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.speech.RecognizerIntent;
import android.view.KeyEvent;
import android.view.View;
import android.view.Window;
import android.view.WindowManager;
import android.webkit.JavascriptInterface;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.Map;

public final class MainActivity extends Activity {
    private static final int VOICE_REQUEST = 41;
    private WebView webView;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        requestWindowFeature(Window.FEATURE_NO_TITLE);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        getWindow().setFlags(
            WindowManager.LayoutParams.FLAG_FULLSCREEN,
            WindowManager.LayoutParams.FLAG_FULLSCREEN
        );

        webView = new WebView(this);
        webView.setBackgroundColor(0xff070b0d);
        webView.setFocusable(true);
        webView.setFocusableInTouchMode(true);
        webView.setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_FULLSCREEN
                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
        );

        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(true);
        settings.setAllowContentAccess(false);
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);

        webView.setWebViewClient(new LocalAssetClient());
        webView.addJavascriptInterface(new PluginBridge(), "TvAgentBridge");
        webView.loadUrl("file:///android_asset/index.html");
        setContentView(webView);
        webView.requestFocus();
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            webView.evaluateJavascript(
                "(window.tvAgentBack && window.tvAgentBack()) === true",
                handled -> {
                    if (!"true".equals(handled)) finish();
                }
            );
            return true;
        }
        String key = keyName(keyCode);
        if (key != null) {
            webView.evaluateJavascript("window.tvAgentKey && window.tvAgentKey(" + quote(key) + ")", null);
            return true;
        }
        return super.onKeyDown(keyCode, event);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != VOICE_REQUEST || resultCode != RESULT_OK || data == null) {
            return;
        }
        ArrayList<String> results = data.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS);
        if (results == null || results.isEmpty()) {
            return;
        }
        webView.evaluateJavascript(
            "window.tvAgentVoiceResult && window.tvAgentVoiceResult(" + quote(results.get(0)) + ")",
            null
        );
    }

    private String keyName(int keyCode) {
        return switch (keyCode) {
            case KeyEvent.KEYCODE_DPAD_LEFT -> "ArrowLeft";
            case KeyEvent.KEYCODE_DPAD_RIGHT -> "ArrowRight";
            case KeyEvent.KEYCODE_DPAD_UP -> "ArrowUp";
            case KeyEvent.KEYCODE_DPAD_DOWN -> "ArrowDown";
            case KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> "Enter";
            case KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE, KeyEvent.KEYCODE_SPACE -> " ";
            case KeyEvent.KEYCODE_ESCAPE -> "Escape";
            default -> null;
        };
    }

    private String quote(String value) {
        StringBuilder output = new StringBuilder(value.length() + 2).append('"');
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
        return output.append('"').toString();
    }

    private final class PluginBridge {
        @JavascriptInterface
        public String request(String method, String path, String body) {
            return PluginTransport.request(getApplicationContext(), method, path, body);
        }

        @JavascriptInterface
        public boolean startVoiceInput() {
            runOnUiThread(() -> {
                Intent intent = new Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH);
                intent.putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM);
                intent.putExtra(RecognizerIntent.EXTRA_LANGUAGE, "zh-CN");
                intent.putExtra(RecognizerIntent.EXTRA_PROMPT, "说出想看的短剧");
                try {
                    startActivityForResult(intent, VOICE_REQUEST);
                } catch (Exception ignored) {
                    // 没有系统语音识别应用时，前端继续使用文本输入。
                }
            });
            PackageManager manager = getPackageManager();
            return manager.hasSystemFeature(PackageManager.FEATURE_MICROPHONE)
                || checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED;
        }
    }

    private final class LocalAssetClient extends WebViewClient {
        @Override
        public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
            String path = request.getUrl().getPath();
            if (path == null || !path.startsWith("/android_asset/")) {
                return null;
            }
            String asset = path.substring("/android_asset/".length());
            String extension = asset.contains(".")
                ? asset.substring(asset.lastIndexOf('.') + 1).toLowerCase()
                : "";
            String mime = switch (extension) {
                case "html" -> "text/html";
                case "css" -> "text/css";
                case "js" -> "application/javascript";
                case "jpg", "jpeg" -> "image/jpeg";
                case "png" -> "image/png";
                case "mp4" -> "video/mp4";
                default -> "application/octet-stream";
            };
            if (!"mp4".equals(extension)) {
                return null;
            }
            try {
                Map<String, String> headers = new HashMap<>();
                headers.put("Content-Type", mime);
                return new WebResourceResponse(
                    mime,
                    null,
                    200,
                    "OK",
                    headers,
                    getAssets().open(asset)
                );
            } catch (Exception ignored) {
                return null;
            }
        }
    }
}
