package site.addzero.tvagent;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.view.View;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Queue;

final class DanmakuOverlay extends View {
    interface TimeSource {
        boolean isPlaying();
        long positionMs();
    }

    static final class Item {
        final long timeMs;
        final int type;
        final int color;
        final String text;

        Item(long timeMs, int type, int color, String text) {
            this.timeMs = timeMs;
            this.type = type;
            this.color = color;
            this.text = text;
        }
    }

    private static final long SCROLL_DURATION_MS = 7000L;
    private static final long FIXED_DURATION_MS = 5000L;
    private static final int MAX_ACTIVE = 80;
    private static final int LANE_HEIGHT_DP = 34;

    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final float density = getResources().getDisplayMetrics().density;
    private final List<Item> items = new ArrayList<>();
    private final Queue<Item> pending = new ArrayDeque<>();
    private final List<Active> active = new ArrayList<>();
    private final long[] laneFreeAt = new long[64];
    private TimeSource timeSource;
    private boolean enabled = true;
    private long lastPosition = -1L;

    DanmakuOverlay(Context context) {
        super(context);
        setLayerType(View.LAYER_TYPE_HARDWARE, null);
        paint.setTextSize(22f * density);
        paint.setTypeface(android.graphics.Typeface.DEFAULT_BOLD);
    }

    void setTimeSource(TimeSource source) {
        timeSource = source;
    }

    void setDanmakuEnabled(boolean value) {
        enabled = value;
        if (!value) {
            active.clear();
        }
        invalidate();
    }

    boolean isDanmakuEnabled() {
        return enabled;
    }

    void setItems(List<Item> loaded) {
        items.clear();
        items.addAll(loaded);
        items.sort(Comparator.comparingLong(item -> item.timeMs));
        reset();
        if (!items.isEmpty()) {
            pending.addAll(items);
        }
    }

    void clearItems() {
        items.clear();
        reset();
    }

    private void reset() {
        pending.clear();
        active.clear();
        java.util.Arrays.fill(laneFreeAt, 0L);
        lastPosition = -1L;
        invalidate();
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        if (!enabled || pending.isEmpty() && active.isEmpty() || timeSource == null) {
            if (timeSource != null && timeSource.isPlaying()) postInvalidateOnAnimation();
            return;
        }
        long now = Math.max(0L, timeSource.positionMs());
        if (lastPosition >= 0L && now < lastPosition) {
            reset();
            pending.addAll(items);
        }
        lastPosition = now;
        spawn(now);
        expire(now);
        drawItems(canvas, now);
        if (timeSource.isPlaying()) postInvalidateOnAnimation();
    }

    private void spawn(long now) {
        while (!pending.isEmpty() && pending.peek().timeMs <= now && active.size() < MAX_ACTIVE) {
            Item item = pending.poll();
            int type = item.type;
            if (type != 1 && type != 4 && type != 5) type = 1;
            paint.setTextSize(22f * density);
            float width = paint.measureText(item.text);
            int lane = findLane(now, type, width);
            if (lane < 0) continue;
            long duration = type == 1
                ? SCROLL_DURATION_MS
                : FIXED_DURATION_MS;
            active.add(new Active(item, lane, width, now, duration));
        }
    }

    private int findLane(long now, int type, float width) {
        int laneCount = Math.max(1, Math.min(laneFreeAt.length, getHeight() / Math.max(1, (int) (LANE_HEIGHT_DP * density))));
        for (int lane = 0; lane < laneCount; lane++) {
            if (laneFreeAt[lane] > now) continue;
            float speed = getWidth() <= 0 ? 0f : (getWidth() + width) / (float) SCROLL_DURATION_MS;
            laneFreeAt[lane] = type == 1
                ? now + (long) (width / Math.max(0.01f, speed))
                : now + FIXED_DURATION_MS;
            return lane;
        }
        return -1;
    }

    private void expire(long now) {
        active.removeIf(item -> now - item.spawnAt > item.durationMs);
    }

    private void drawItems(Canvas canvas, long now) {
        float laneHeight = LANE_HEIGHT_DP * density;
        for (Active item : active) {
            paint.setTextSize(22f * density);
            float x;
            float y;
            if (item.data.type == 4) {
                x = (getWidth() - item.width) / 2f;
                y = item.lane * laneHeight + paint.getTextSize();
            } else if (item.data.type == 5) {
                x = (getWidth() - item.width) / 2f;
                y = getHeight() - (item.lane + 1) * laneHeight + paint.getTextSize();
            } else {
                float progress = (now - item.spawnAt) / (float) item.durationMs;
                x = getWidth() - progress * (getWidth() + item.width);
                y = item.lane * laneHeight + paint.getTextSize();
            }
            paint.setStyle(Paint.Style.STROKE);
            paint.setStrokeWidth(3f * density);
            paint.setColor(Color.argb(220, 0, 0, 0));
            canvas.drawText(item.data.text, x, y, paint);
            paint.setStyle(Paint.Style.FILL);
            paint.setColor(item.data.color);
            canvas.drawText(item.data.text, x, y, paint);
        }
    }

    private static final class Active {
        final Item data;
        final int lane;
        final float width;
        final long spawnAt;
        final long durationMs;

        Active(Item data, int lane, float width, long spawnAt, long durationMs) {
            this.data = data;
            this.lane = lane;
            this.width = width;
            this.spawnAt = spawnAt;
            this.durationMs = durationMs;
        }
    }
}
