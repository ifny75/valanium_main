package app.valanium;

import android.app.Activity;
import android.app.Dialog;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.drawable.GradientDrawable;
import android.view.GestureDetector;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.ScaleGestureDetector;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowManager;
import android.widget.FrameLayout;
import android.widget.TextView;

/**
 * Полноэкранный просмотр расшифрованного изображения без временных файлов.
 * Bitmap приходит из памяти пузыря или профиля и не покидает процесс.
 */
final class PhotoViewer {
    private final Activity activity;
    private final Bitmap bitmap;

    PhotoViewer(Activity activity, Bitmap bitmap) {
        this.activity = activity;
        this.bitmap = bitmap;
    }

    void show() {
        Dialog dialog = new Dialog(activity, android.R.style.Theme_Material_NoActionBar);
        Window window = dialog.getWindow();
        if (window == null) return;
        window.setBackgroundDrawableResource(android.R.color.transparent);
        window.setStatusBarColor(Color.BLACK);
        window.setNavigationBarColor(Color.BLACK);
        if ((activity.getWindow().getAttributes().flags
                & WindowManager.LayoutParams.FLAG_SECURE) != 0) {
            window.addFlags(WindowManager.LayoutParams.FLAG_SECURE);
        }

        FrameLayout root = new FrameLayout(activity);
        root.setBackgroundColor(Color.rgb(3, 3, 4));

        ZoomView image = new ZoomView(activity, bitmap);
        image.setContentDescription("Просмотр изображения. Сведите пальцы для масштаба");
        root.addView(image, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));

        TextView close = new TextView(activity);
        close.setText("×");
        close.setTextColor(Color.WHITE);
        close.setTextSize(30);
        close.setGravity(Gravity.CENTER);
        close.setContentDescription("Закрыть просмотр");
        close.setBackground(round(0xCC171719, 0x553F3F44));
        close.setOnClickListener(v -> dialog.dismiss());
        FrameLayout.LayoutParams closeParams = new FrameLayout.LayoutParams(dp(48), dp(48),
                Gravity.TOP | Gravity.END);
        closeParams.setMargins(0, dp(20), dp(16), 0);
        root.addView(close, closeParams);

        TextView hint = new TextView(activity);
        hint.setText("Двойное касание · масштаб пальцами");
        hint.setTextColor(0xFFB4AFBE);
        hint.setTextSize(11);
        hint.setGravity(Gravity.CENTER);
        hint.setPadding(dp(14), dp(8), dp(14), dp(8));
        hint.setBackground(round(0xB8171719, 0x443F3F44));
        FrameLayout.LayoutParams hintParams = new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM | Gravity.CENTER_HORIZONTAL);
        hintParams.bottomMargin = dp(28);
        root.addView(hint, hintParams);

        dialog.setContentView(root);
        dialog.setOnShowListener(ignored -> window.setLayout(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        dialog.show();
    }

    private GradientDrawable round(int fill, int stroke) {
        GradientDrawable shape = new GradientDrawable();
        shape.setColor(fill);
        shape.setStroke(dp(1), stroke);
        shape.setCornerRadius(dp(999));
        return shape;
    }

    private int dp(int value) {
        return Math.round(value * activity.getResources().getDisplayMetrics().density);
    }

    /** FIT_CENTER с управляемыми масштабом и смещением, без копии Bitmap. */
    private static final class ZoomView extends View {
        private final Bitmap bitmap;
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
        private final ScaleGestureDetector scaleDetector;
        private final GestureDetector gestureDetector;
        private float scale = 1f;
        private float offsetX;
        private float offsetY;

        ZoomView(Activity activity, Bitmap bitmap) {
            super(activity);
            this.bitmap = bitmap;
            setFocusable(true);
            scaleDetector = new ScaleGestureDetector(activity,
                    new ScaleGestureDetector.SimpleOnScaleGestureListener() {
                        @Override public boolean onScale(ScaleGestureDetector detector) {
                            scale = clamp(scale * detector.getScaleFactor(), 1f, 5f);
                            constrain();
                            invalidate();
                            return true;
                        }
                    });
            gestureDetector = new GestureDetector(activity,
                    new GestureDetector.SimpleOnGestureListener() {
                        @Override public boolean onDown(MotionEvent event) { return true; }
                        @Override public boolean onScroll(MotionEvent first, MotionEvent current,
                                float distanceX, float distanceY) {
                            if (scale <= 1f) return false;
                            offsetX -= distanceX;
                            offsetY -= distanceY;
                            constrain();
                            invalidate();
                            return true;
                        }
                        @Override public boolean onDoubleTap(MotionEvent event) {
                            scale = scale > 1f ? 1f : 2.5f;
                            if (scale == 1f) { offsetX = 0; offsetY = 0; }
                            constrain();
                            invalidate();
                            return true;
                        }
                    });
        }

        @Override protected void onDraw(Canvas canvas) {
            super.onDraw(canvas);
            if (bitmap.getWidth() == 0 || bitmap.getHeight() == 0) return;
            float fit = Math.min((float) getWidth() / bitmap.getWidth(),
                    (float) getHeight() / bitmap.getHeight());
            float width = bitmap.getWidth() * fit * scale;
            float height = bitmap.getHeight() * fit * scale;
            float left = (getWidth() - width) / 2f + offsetX;
            float top = (getHeight() - height) / 2f + offsetY;
            canvas.drawBitmap(bitmap, null, new RectF(left, top, left + width, top + height), paint);
        }

        @Override public boolean onTouchEvent(MotionEvent event) {
            scaleDetector.onTouchEvent(event);
            gestureDetector.onTouchEvent(event);
            return true;
        }

        private void constrain() {
            if (scale <= 1f || getWidth() == 0 || getHeight() == 0) {
                offsetX = 0;
                offsetY = 0;
                return;
            }
            float fit = Math.min((float) getWidth() / bitmap.getWidth(),
                    (float) getHeight() / bitmap.getHeight());
            float maxX = Math.max(0, (bitmap.getWidth() * fit * scale - getWidth()) / 2f);
            float maxY = Math.max(0, (bitmap.getHeight() * fit * scale - getHeight()) / 2f);
            offsetX = clamp(offsetX, -maxX, maxX);
            offsetY = clamp(offsetY, -maxY, maxY);
        }

        private static float clamp(float value, float min, float max) {
            return Math.max(min, Math.min(max, value));
        }
    }
}
