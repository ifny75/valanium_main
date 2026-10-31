package app.valanium;

import android.graphics.*;
import android.graphics.drawable.Drawable;

final class AvatarDrawable extends Drawable {
    private final Bitmap bitmap;
    private final BitmapShader shader;
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
    private final float radius;

    AvatarDrawable(Bitmap bitmap, float radius) {
        this.bitmap = bitmap;
        this.radius = radius;
        shader = new BitmapShader(bitmap, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP);
        paint.setShader(shader);
    }

    @Override protected void onBoundsChange(Rect bounds) {
        if (bounds.isEmpty()) return;
        float[] fit = AvatarTransform.fit(bitmap.getWidth(), bitmap.getHeight(), bounds.width(), bounds.height());
        Matrix matrix = new Matrix();
        matrix.setScale(fit[0], fit[0]);
        matrix.postTranslate(bounds.left + fit[1], bounds.top + fit[2]);
        shader.setLocalMatrix(matrix);
    }

    @Override public void draw(Canvas canvas) { canvas.drawRoundRect(new RectF(getBounds()), radius, radius, paint); }
    @Override public void setAlpha(int alpha) { paint.setAlpha(alpha); invalidateSelf(); }
    @Override public void setColorFilter(ColorFilter filter) { paint.setColorFilter(filter); invalidateSelf(); }
    @Override public int getOpacity() { return PixelFormat.TRANSLUCENT; }
}
