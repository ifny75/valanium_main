package app.valanium;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.drawable.Drawable;
import android.util.AttributeSet;
import android.view.View;
import android.widget.FrameLayout;

/**
 * Панель, размывающая то, что лежит под ней.
 *
 * Android не даёт «размытия подложки» готовым свойством: RenderEffect размывает
 * только сам элемент, а setBackgroundBlurRadius — окно целиком. Поэтому кусок
 * экрана под островком снимается в маленькую картинку, размывается двумя
 * проходами и рисуется обратно растянутым. Уменьшение оставляет эффект дешёвым,
 * а отдельный box blur не даёт панели выглядеть обычной прозрачностью.
 *
 * Через аппаратный слой это сделать не вышло: перерисовка чужого элемента
 * внутри своего кадра упирается в «Recording currently in progress» — display
 * list этого элемента уже пишется. Софтверный холст такой петли не создаёт.
 */
public class BlurPanel extends FrameLayout {
    private static final float RADIUS_DP = 26f;
    /** Во сколько раз уменьшать снимок. */
    private static final int SCALE = 4;
    /** Радиус каждого прохода на уменьшенном снимке. */
    private static final int BLUR_RADIUS = 4;

    private final Paint paint = new Paint(Paint.FILTER_BITMAP_FLAG | Paint.ANTI_ALIAS_FLAG);
    private final Path clip = new Path();
    private final Rect source_rect = new Rect();
    private final RectF target = new RectF();
    private final int[] panelLocation = new int[2];
    private final int[] sourceLocation = new int[2];
    private final int[] rootLocation = new int[2];

    private Bitmap shot;
    private Canvas shotCanvas;
    private Bitmap viewport;
    private Canvas viewportCanvas;
    private boolean backdropDirty = true;
    private final Runnable refreshBackdrop = () -> {
        backdropDirty = true;
        invalidate();
    };
    private int[] pixels;
    private int[] scratch;

    /** Что размывать: экран под островком. */
    private View source;
    private int accent = Color.rgb(151, 112, 255);

    public void setAccent(int color) {
        accent = color;
        backdropDirty = true;
        invalidate();
    }

    public BlurPanel(Context context) { this(context, null); }

    public BlurPanel(Context context, AttributeSet attrs) {
        super(context, attrs);
        setWillNotDraw(false);
    }

    public void setSource(View source) {
        removeCallbacks(refreshBackdrop);
        this.source = source;
        backdropDirty = true;
        invalidate();
    }

    /** Keep the last glass image during gestures/flings; refresh after idle. */
    public void onSourceScrolled() {
        removeCallbacks(refreshBackdrop);
        postDelayed(refreshBackdrop, 160);
    }

    @Override
    protected void onDetachedFromWindow() {
        removeCallbacks(refreshBackdrop);
        super.onDetachedFromWindow();
    }

    @Override
    public void draw(Canvas canvas) {
        // Сначала фон под островком, затем его полупрозрачное стекло и дети.
        // В dispatchDraw фон View уже нарисован, поэтому снимок раньше стирал
        // тонировку island.xml и панель выглядела просто прозрачной.
        drawBackdrop(canvas);
        super.draw(canvas);
    }

    private void drawBackdrop(Canvas canvas) {
        View behind = source;
        if (behind == null || behind.getVisibility() != View.VISIBLE) return;

        int width = getWidth();
        int height = getHeight();
        if (width == 0 || height == 0) return;

        int small = Math.max(1, width / SCALE);
        int tall = Math.max(1, height / SCALE);
        boolean resized = shot == null || shot.getWidth() != small || shot.getHeight() != tall;
        if (backdropDirty || resized) {
        if (shot == null || shot.getWidth() != small || shot.getHeight() != tall) {
            shot = Bitmap.createBitmap(small, tall, Bitmap.Config.ARGB_8888);
            shotCanvas = new Canvas(shot);
            pixels = new int[small * tall];
            scratch = new int[small * tall];
        }

        // Снимок обязан быть непрозрачным. Иначе через прозрачные пиксели
        // размытого слоя продолжает просвечивать исходный резкий текст.
        shot.eraseColor(Color.BLACK);
        View root = getParent() instanceof View ? (View) getParent() : null;
        Drawable rootBackground = root == null ? null : root.getBackground();
        if (rootBackground != null) {
            getLocationInWindow(panelLocation);
            root.getLocationInWindow(rootLocation);
            shotCanvas.save();
            shotCanvas.scale(1f / SCALE, 1f / SCALE);
            shotCanvas.translate(rootLocation[0] - panelLocation[0],
                    rootLocation[1] - panelLocation[1]);
            rootBackground.setBounds(0, 0, root.getWidth(), root.getHeight());
            rootBackground.draw(shotCanvas);
            shotCanvas.restore();
        }

        getLocationInWindow(panelLocation);
        behind.getLocationInWindow(sourceLocation);
        // Draw with the entire visible viewport as the clip. Drawing the source
        // into the tiny island clip marks other children quick-rejected, which
        // can leave their hardware display lists missing during scrolling.
        int viewportWidth = Math.max(1, (behind.getWidth() + SCALE - 1) / SCALE);
        int viewportHeight = Math.max(1, (behind.getHeight() + SCALE - 1) / SCALE);
        if (viewport == null || viewport.getWidth() != viewportWidth
                || viewport.getHeight() != viewportHeight) {
            viewport = Bitmap.createBitmap(viewportWidth, viewportHeight, Bitmap.Config.ARGB_8888);
            viewportCanvas = new Canvas(viewport);
        }
        viewport.eraseColor(Color.TRANSPARENT);
        viewportCanvas.save();
        viewportCanvas.scale(1f / SCALE, 1f / SCALE);
        viewportCanvas.translate(-behind.getScrollX(), -behind.getScrollY());
        try {
            behind.draw(viewportCanvas);
        } catch (RuntimeException error) {
            // Не отрисовалось — останется просто стекло. Ронять приложение
            // из-за оформления нельзя.
            viewportCanvas.restore();
            return;
        }
        viewportCanvas.restore();
        shotCanvas.drawBitmap(viewport,
                (sourceLocation[0] - panelLocation[0]) / (float) SCALE,
                (sourceLocation[1] - panelLocation[1]) / (float) SCALE, paint);
        blurShot(small, tall);
        backdropDirty = false;
        }

        float radius = RADIUS_DP * getResources().getDisplayMetrics().density;
        clip.reset();
        clip.addRoundRect(0, 0, width, height, radius, radius, Path.Direction.CW);
        source_rect.set(0, 0, small, tall);
        target.set(0, 0, width, height);

        canvas.save();
        canvas.clipPath(clip);
        canvas.drawBitmap(shot, source_rect, target, paint);
        paint.setColor(Color.argb(30, Color.red(accent), Color.green(accent), Color.blue(accent)));
        canvas.drawRect(target, paint);
        paint.setColor(Color.WHITE);
        canvas.restore();
    }

    /**
     * Два прохода box blur дают мягкое размытие, близкое к Gaussian, без
     * RenderScript и сторонних зависимостей. Картинка маленькая, поэтому даже
     * простой проход остаётся дешёвым.
     */
    private void blurShot(int width, int height) {
        shot.getPixels(pixels, 0, width, 0, 0, width, height);
        int diameter = BLUR_RADIUS * 2 + 1;

        for (int y = 0; y < height; y++) {
            int row = y * width;
            for (int x = 0; x < width; x++) {
                int alpha = 0, red = 0, green = 0, blue = 0;
                for (int offset = -BLUR_RADIUS; offset <= BLUR_RADIUS; offset++) {
                    int sample = pixels[row + Math.max(0, Math.min(width - 1, x + offset))];
                    alpha += sample >>> 24;
                    red += (sample >>> 16) & 0xff;
                    green += (sample >>> 8) & 0xff;
                    blue += sample & 0xff;
                }
                scratch[row + x] = Color.argb(alpha / diameter, red / diameter,
                        green / diameter, blue / diameter);
            }
        }

        for (int y = 0; y < height; y++) {
            for (int x = 0; x < width; x++) {
                int alpha = 0, red = 0, green = 0, blue = 0;
                for (int offset = -BLUR_RADIUS; offset <= BLUR_RADIUS; offset++) {
                    int sampleY = Math.max(0, Math.min(height - 1, y + offset));
                    int sample = scratch[sampleY * width + x];
                    alpha += sample >>> 24;
                    red += (sample >>> 16) & 0xff;
                    green += (sample >>> 8) & 0xff;
                    blue += sample & 0xff;
                }
                pixels[y * width + x] = Color.argb(alpha / diameter, red / diameter,
                        green / diameter, blue / diameter);
            }
        }
        shot.setPixels(pixels, 0, width, 0, 0, width, height);
    }
}
