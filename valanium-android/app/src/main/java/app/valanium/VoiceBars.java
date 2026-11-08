package app.valanium;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.os.SystemClock;
import android.view.View;

/** Five rounded bars that rise with a live voice level (0..1) and settle when it is quiet. */
final class VoiceBars extends View {
    interface Level { float get(); }
    private static final float[] SHAPE = {0.55f, 0.85f, 1f, 0.8f, 0.5f};
    private final Level level;
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final float[] heights = new float[SHAPE.length];
    private final RectF bar = new RectF();
    private final boolean motion;

    VoiceBars(Context context, Level level, int color) {
        super(context);
        this.level = level;
        paint.setColor(color);
        motion = android.provider.Settings.Global.getFloat(context.getContentResolver(),
                android.provider.Settings.Global.ANIMATOR_DURATION_SCALE, 1f) != 0f;
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
    }

    void setColor(int color) { paint.setColor(color); invalidate(); }

    @Override protected void onDraw(Canvas canvas) {
        float value = Math.max(0f, Math.min(1f, level.get()));
        float time = SystemClock.uptimeMillis() / 1000f;
        int count = SHAPE.length;
        float width = getWidth(), height = getHeight();
        float gap = width / (count * 2.6f);
        float thickness = (width - gap * (count - 1)) / count;
        for (int i = 0; i < count; i++) {
            // A little independent movement per bar, only while someone is speaking.
            float wobble = motion ? 0.22f * (float) Math.sin(time * (7 + i * 1.7) + i * 1.3) * value : 0f;
            float target = 0.2f + 0.8f * Math.max(0f, Math.min(1f, value * SHAPE[i] * 1.25f + wobble));
            heights[i] += (target - heights[i]) * (motion ? 0.3f : 1f);
            float h = Math.max(thickness, height * heights[i]);
            float left = i * (thickness + gap);
            bar.set(left, (height - h) / 2f, left + thickness, (height + h) / 2f);
            canvas.drawRoundRect(bar, thickness / 2f, thickness / 2f, paint);
        }
        if (isShown()) postInvalidateOnAnimation();
    }
}
