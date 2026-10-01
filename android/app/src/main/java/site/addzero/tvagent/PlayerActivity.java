package site.addzero.tvagent;

import android.app.Activity;
import android.graphics.Color;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowManager;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import android.widget.Toast;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import androidx.media3.common.C;
import androidx.media3.common.MediaItem;
import androidx.media3.common.PlaybackException;
import androidx.media3.common.Player;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.ui.PlayerView;

public final class PlayerActivity extends Activity {
    public static final String EXTRA_URL = "url";
    public static final String EXTRA_TITLE = "title";
    public static final String EXTRA_SUBTITLE = "subtitle";

    private static final long CONTROLS_VISIBLE_MS = 4500L;
    private static final long PROGRESS_REFRESH_MS = 500L;
    private static final long SEEK_STEP_MS = 15_000L;
    private static final int HORIZONTAL_PADDING = 34;
    private static final int VERTICAL_PADDING = 28;
    private static final int BUTTON_WIDTH = 108;
    private static final int BUTTON_HEIGHT = 56;
    private static final int CONTROL_GAP = 16;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Runnable hideControlsTask = this::hideControls;
    private final Runnable refreshProgressTask = this::refreshProgress;
    private final float[] speeds = {0.5f, 0.75f, 1.0f, 1.25f, 1.5f, 2.0f};
    private final ExecutorService danmakuExecutor = Executors.newSingleThreadExecutor();
    private int speedIndex = 2;
    private boolean danmakuEnabled = true;

    private ExoPlayer player;
    private PlayerView playerView;
    private DanmakuOverlay danmakuOverlay;
    private LinearLayout controls;
    private LinearLayout topBar;
    private TextView playPauseButton;
    private TextView rewindButton;
    private TextView forwardButton;
    private TextView speedButton;
    private TextView danmakuButton;
    private TextView fullscreenButton;
    private TextView positionView;
    private TextView durationView;
    private ProgressBar progress;
    private Object backRegistration;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        requestWindowFeature(Window.FEATURE_NO_TITLE);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        enterImmersiveMode();

        String url = getIntent().getStringExtra(EXTRA_URL);
        String title = getIntent().getStringExtra(EXTRA_TITLE);
        String subtitle = getIntent().getStringExtra(EXTRA_SUBTITLE);
        if (url == null || (!url.startsWith("https://") && !url.startsWith("http://"))) {
            finish();
            return;
        }

        player = new ExoPlayer.Builder(this).build();
        player.addListener(new Player.Listener() {
            @Override
            public void onPlayerError(PlaybackException error) {
                Toast.makeText(PlayerActivity.this, "视频播放失败：" + error.getMessage(), Toast.LENGTH_LONG).show();
                showControls();
            }

            @Override
            public void onIsPlayingChanged(boolean isPlaying) {
                playPauseButton.setText(isPlaying ? "暂停" : "播放");
                if (isPlaying) {
                    if (danmakuOverlay != null) {
                        danmakuOverlay.invalidate();
                    }
                    scheduleControlsHide();
                } else {
                    showControls();
                }
            }
        });

        playerView = new PlayerView(this);
        playerView.setBackgroundColor(Color.BLACK);
        playerView.setKeepScreenOn(true);
        playerView.setUseController(false);
        playerView.setFocusable(false);
        playerView.setPlayer(player);

        danmakuOverlay = new DanmakuOverlay(this);
        danmakuOverlay.setFocusable(false);
        danmakuOverlay.setTimeSource(new DanmakuOverlay.TimeSource() {
            @Override
            public boolean isPlaying() {
                return player != null && player.isPlaying();
            }

            @Override
            public long positionMs() {
                return player == null ? 0L : player.getCurrentPosition();
            }
        });
        danmakuOverlay.setDanmakuEnabled(true);

        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(Color.BLACK);
        root.addView(playerView, matchParent());
        root.addView(danmakuOverlay, matchParent());
        root.addView(buildTopBar(title, subtitle));
        root.addView(buildControls());
        setContentView(root);

        player.setMediaItem(MediaItem.fromUri(url));
        player.setPlayWhenReady(true);
        player.prepare();
        playPauseButton.requestFocus();
        backRegistration = BackNavigation.register(this, this::handleBack);
        handler.post(refreshProgressTask);
        showControls();
        loadDanmaku(title, subtitle);
    }

    private FrameLayout.LayoutParams matchParent() {
        return new FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT
        );
    }

    private View buildTopBar(String title, String subtitle) {
        topBar = new LinearLayout(this);
        topBar.setOrientation(LinearLayout.VERTICAL);
        topBar.setGravity(Gravity.START);
        topBar.setPadding(HORIZONTAL_PADDING, VERTICAL_PADDING, HORIZONTAL_PADDING, 0);
        topBar.setBackgroundColor(Color.argb(180, 0, 0, 0));

        TextView titleView = new TextView(this);
        titleView.setTextColor(Color.WHITE);
        titleView.setTextSize(22);
        titleView.setText(title == null || title.isBlank() ? "视频播放" : title);
        topBar.addView(titleView);

        if (subtitle != null && !subtitle.isBlank()) {
            TextView subtitleView = new TextView(this);
            subtitleView.setTextColor(Color.rgb(176, 187, 183));
            subtitleView.setTextSize(14);
            subtitleView.setText(subtitle);
            LinearLayout.LayoutParams subtitleParams = wrapContent();
            subtitleParams.topMargin = 4;
            topBar.addView(subtitleView, subtitleParams);
        }

        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            Gravity.TOP
        );
        topBar.setLayoutParams(params);
        return topBar;
    }

    private View buildControls() {
        controls = new LinearLayout(this);
        controls.setOrientation(LinearLayout.HORIZONTAL);
        controls.setGravity(Gravity.CENTER_VERTICAL);
        controls.setPadding(HORIZONTAL_PADDING, 0, HORIZONTAL_PADDING, VERTICAL_PADDING);
        controls.setBackgroundColor(Color.argb(180, 0, 0, 0));

        playPauseButton = actionButton("暂停");
        playPauseButton.setOnClickListener(view -> togglePlayback());
        controls.addView(playPauseButton);

        rewindButton = actionButton("-15秒");
        rewindButton.setOnClickListener(view -> seekBy(-SEEK_STEP_MS));
        controls.addView(rewindButton);

        forwardButton = actionButton("+15秒");
        forwardButton.setOnClickListener(view -> seekBy(SEEK_STEP_MS));
        controls.addView(forwardButton);

        positionView = timelineText("00:00");
        controls.addView(positionView);

        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(1000);
        progress.setProgress(0);
        LinearLayout.LayoutParams progressParams = new LinearLayout.LayoutParams(0, 10, 1f);
        progressParams.leftMargin = CONTROL_GAP;
        progressParams.rightMargin = CONTROL_GAP;
        controls.addView(progress, progressParams);

        durationView = timelineText("00:00");
        controls.addView(durationView);

        speedButton = actionButton("1.0x");
        speedButton.setOnClickListener(view -> cyclePlaybackSpeed());
        controls.addView(speedButton);

        danmakuButton = actionButton("弹幕开");
        danmakuButton.setOnClickListener(view -> toggleDanmaku());
        controls.addView(danmakuButton);

        fullscreenButton = actionButton("退出全屏");
        fullscreenButton.setOnClickListener(view -> toggleImmersiveMode());
        LinearLayout.LayoutParams fullscreenParams = buttonParams();
        fullscreenParams.leftMargin = CONTROL_GAP;
        fullscreenButton.setLayoutParams(fullscreenParams);
        controls.addView(fullscreenButton);

        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            Gravity.BOTTOM
        );
        controls.setLayoutParams(params);
        return controls;
    }

    private LinearLayout.LayoutParams wrapContent() {
        return new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.WRAP_CONTENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
    }

    private LinearLayout.LayoutParams buttonParams() {
        return new LinearLayout.LayoutParams(BUTTON_WIDTH, BUTTON_HEIGHT);
    }

    private TextView actionButton(String text) {
        TextView button = new TextView(this);
        button.setText(text);
        button.setTextColor(Color.WHITE);
        button.setTextSize(16);
        button.setGravity(Gravity.CENTER);
        button.setFocusable(true);
        button.setClickable(true);
        button.setBackgroundColor(Color.rgb(16, 24, 25));
        button.setLayoutParams(buttonParams());
        button.setOnFocusChangeListener((view, hasFocus) -> {
            view.setBackgroundColor(hasFocus ? Color.rgb(215, 255, 100) : Color.rgb(16, 24, 25));
            ((TextView) view).setTextColor(hasFocus ? Color.rgb(7, 11, 13) : Color.WHITE);
        });
        return button;
    }

    private TextView timelineText(String text) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextColor(Color.rgb(195, 205, 200));
        view.setTextSize(14);
        view.setGravity(Gravity.CENTER);
        return view;
    }

    private void togglePlayback() {
        if (player == null) {
            return;
        }
        if (player.isPlaying()) {
            player.pause();
        } else {
            player.play();
        }
        showControls();
    }

    private void seekBy(long deltaMs) {
        if (player == null) {
            return;
        }
        long duration = Math.max(0L, player.getDuration());
        long target = Math.max(0L, player.getCurrentPosition() + deltaMs);
        if (duration != C.TIME_UNSET && duration > 0) {
            target = Math.min(target, duration);
        }
        player.seekTo(target);
        showControls();
    }

    private void cyclePlaybackSpeed() {
        if (player == null) {
            return;
        }
        speedIndex = (speedIndex + 1) % speeds.length;
        float speed = speeds[speedIndex];
        player.setPlaybackSpeed(speed);
        speedButton.setText(String.format(Locale.ROOT, "%.2fx", speed));
        showControls();
    }

    private void toggleDanmaku() {
        danmakuEnabled = !danmakuEnabled;
        danmakuOverlay.setDanmakuEnabled(danmakuEnabled);
        danmakuButton.setText(danmakuEnabled ? "弹幕开" : "弹幕关");
        Toast.makeText(this, danmakuEnabled ? "弹幕已开启" : "弹幕已关闭", Toast.LENGTH_SHORT).show();
        showControls();
    }

    private void loadDanmaku(String title, String subtitle) {
        if (title == null || title.isBlank()) {
            return;
        }
        if (DanmakuClient.configuredApi(this).isEmpty()) {
            Toast.makeText(this, "未配置弹幕接口，可在模型设置中填写", Toast.LENGTH_LONG).show();
            return;
        }
        String episode = episodeName(subtitle);
        danmakuExecutor.execute(() -> {
            try {
                List<DanmakuOverlay.Item> items = DanmakuClient.load(this, title, episode);
                runOnUiThread(() -> {
                    if (isFinishing() || isDestroyed()) {
                        return;
                    }
                    danmakuOverlay.setItems(items);
                    if (items.isEmpty()) {
                        Toast.makeText(this, "未找到本集弹幕", Toast.LENGTH_SHORT).show();
                    }
                });
            } catch (Exception error) {
                runOnUiThread(() -> {
                    if (!isFinishing() && !isDestroyed()) {
                        Toast.makeText(this, "弹幕加载失败：" + error.getMessage(), Toast.LENGTH_LONG).show();
                    }
                });
            }
        });
    }

    private String episodeName(String subtitle) {
        if (subtitle == null) {
            return "";
        }
        int separator = subtitle.indexOf('·');
        return separator < 0 ? subtitle.trim() : subtitle.substring(0, separator).trim();
    }

    private void toggleImmersiveMode() {
        if (isImmersiveMode()) {
            exitImmersiveMode();
        } else {
            enterImmersiveMode();
        }
        updateFullscreenLabel();
        showControls();
    }

    private boolean isImmersiveMode() {
        return (getWindow().getDecorView().getSystemUiVisibility() & View.SYSTEM_UI_FLAG_FULLSCREEN) != 0;
    }

    private void enterImmersiveMode() {
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
        getWindow().getDecorView().setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_FULLSCREEN
                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
        );
        updateFullscreenLabel();
    }

    private void exitImmersiveMode() {
        getWindow().clearFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
        getWindow().getDecorView().setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
        );
        updateFullscreenLabel();
    }

    private void updateFullscreenLabel() {
        if (fullscreenButton != null) {
            fullscreenButton.setText(isImmersiveMode() ? "退出全屏" : "全屏");
        }
    }

    private void showControls() {
        handler.removeCallbacks(hideControlsTask);
        if (topBar != null) {
            topBar.setVisibility(View.VISIBLE);
        }
        if (controls != null) {
            controls.setVisibility(View.VISIBLE);
        }
        scheduleControlsHide();
    }

    private void scheduleControlsHide() {
        handler.removeCallbacks(hideControlsTask);
        if (player != null && player.isPlaying()) {
            handler.postDelayed(hideControlsTask, CONTROLS_VISIBLE_MS);
        }
    }

    private void hideControls() {
        if (player != null && player.isPlaying()) {
            topBar.setVisibility(View.GONE);
            controls.setVisibility(View.GONE);
        }
    }

    private void refreshProgress() {
        if (player == null) {
            return;
        }
        long duration = Math.max(0L, player.getDuration());
        long position = Math.max(0L, player.getCurrentPosition());
        positionView.setText(formatTime(position));
        durationView.setText(duration > 0 ? formatTime(duration) : "00:00");
        progress.setProgress(duration > 0 ? (int) Math.min(1000L, position * 1000L / duration) : 0);
        handler.postDelayed(refreshProgressTask, PROGRESS_REFRESH_MS);
    }

    private String formatTime(long millis) {
        long totalSeconds = millis / 1000L;
        long hours = totalSeconds / 3600L;
        long minutes = totalSeconds % 3600L / 60L;
        long seconds = totalSeconds % 60L;
        if (hours > 0) {
            return String.format(Locale.ROOT, "%d:%02d:%02d", hours, minutes, seconds);
        }
        return String.format(Locale.ROOT, "%02d:%02d", minutes, seconds);
    }

    private void handleBack() {
        if (controls == null || controls.getVisibility() != View.VISIBLE) {
            showControls();
            return;
        }
        if (!isImmersiveMode()) {
            enterImmersiveMode();
            showControls();
            return;
        }
        finish();
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (keyCode == KeyEvent.KEYCODE_BACK || keyCode == KeyEvent.KEYCODE_ESCAPE) {
            handleBack();
            return true;
        }
        if (keyCode == KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE || keyCode == KeyEvent.KEYCODE_SPACE) {
            togglePlayback();
            return true;
        }
        if (keyCode == KeyEvent.KEYCODE_MEDIA_REWIND) {
            seekBy(-SEEK_STEP_MS);
            return true;
        }
        if (keyCode == KeyEvent.KEYCODE_MEDIA_FAST_FORWARD) {
            seekBy(SEEK_STEP_MS);
            return true;
        }
        if (controls.getVisibility() != View.VISIBLE && isDirectionKey(keyCode)) {
            showControls();
            playPauseButton.requestFocus();
            return true;
        }
        if (keyCode == KeyEvent.KEYCODE_DPAD_CENTER
            || keyCode == KeyEvent.KEYCODE_ENTER
            || keyCode == KeyEvent.KEYCODE_NUMPAD_ENTER) {
            showControls();
        }
        return super.onKeyDown(keyCode, event);
    }

    private boolean isDirectionKey(int keyCode) {
        return keyCode == KeyEvent.KEYCODE_DPAD_LEFT
            || keyCode == KeyEvent.KEYCODE_DPAD_RIGHT
            || keyCode == KeyEvent.KEYCODE_DPAD_UP
            || keyCode == KeyEvent.KEYCODE_DPAD_DOWN;
    }

    @Override
    public boolean onKeyUp(int keyCode, KeyEvent event) {
        if (keyCode == KeyEvent.KEYCODE_BACK || keyCode == KeyEvent.KEYCODE_ESCAPE) {
            return true;
        }
        return super.onKeyUp(keyCode, event);
    }

    @Override
    protected void onPause() {
        super.onPause();
        if (player != null) {
            player.pause();
        }
    }

    @Override
    protected void onDestroy() {
        handler.removeCallbacks(hideControlsTask);
        handler.removeCallbacks(refreshProgressTask);
        danmakuExecutor.shutdownNow();
        BackNavigation.unregister(this, backRegistration);
        backRegistration = null;
        if (player != null) {
            player.release();
            player = null;
        }
        super.onDestroy();
    }
}
