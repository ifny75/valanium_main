package app.valanium;

import android.app.Activity;
import android.app.AlertDialog;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.ColorMatrix;
import android.graphics.ColorMatrixColorFilter;
import android.graphics.Matrix;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.RectF;
import android.util.Base64;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.ScaleGestureDetector;
import android.view.GestureDetector;
import android.widget.ScrollView;
import android.view.View;
import android.widget.Button;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.SeekBar;
import android.widget.TextView;

import java.io.ByteArrayOutputStream;

/**
 * Правка снимка перед отправкой: кадр, поворот, отражение, тон.
 *
 * Всё считается на месте, в {@link Bitmap}. Исходник никуда не уходит и нигде не
 * остаётся: наружу выходит только пережатый JPEG, который и попадает в
 * шифрованное сообщение. Метаданные снимка — EXIF с геопозицией, моделью камеры
 * и временем — при этом теряются. Это не побочный эффект, а одна из причин гнать
 * фотографию через перерисовку, а не отправлять файл как есть.
 *
 * <p>Класс намеренно ничего не знает ни о сети, ни о ядре: он получает исходник
 * и отдаёт base64 через {@link Result}.
 */
final class PhotoEditor {

    /** Длинная сторона рабочего изображения. Больше в переписке не нужно. */
    private static final int WORK_MAX_SIDE = 1600;
    /** Минимальная сторона кадра: меньше уже не кадрирование, а промах. */
    private static final int MIN_CROP = 48;

    interface Result {
        void ready(String base64, int width, int height);
    }

    private final Activity activity;
    private final Bitmap source;
    private final int maxBase64;
    private final Result result;

    /** Исходник, уже повёрнутый и отражённый. */
    private Bitmap work;
    /** Кадр в координатах {@link #work}. */
    private RectF crop;
    private int rotation;
    private boolean flipped;
    /** 0 — свободное соотношение. */
    private float ratio;
    private int quality = 82;
    private int brightness = 100;
    private int contrast = 100;
    private int saturation = 100;

    private CropView view;
    private TextView size;
    private Button send;
    private SeekBar zoomBar;
    private TextView zoomLabel;

    PhotoEditor(Activity activity, Bitmap source, float ratio, int maxBase64, Result result) {
        this.activity = activity;
        this.source = source;
        this.ratio = ratio;
        this.maxBase64 = maxBase64;
        this.result = result;
    }

    void show() {
        rebuildWork();
        resetCrop();

        LinearLayout root = new LinearLayout(activity);
        root.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(14);
        root.setPadding(pad, pad, pad, pad);

        view = new CropView(activity);
        LinearLayout.LayoutParams viewParams =
                new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(280));
        root.addView(view, viewParams);

        TextView hint = new TextView(activity);
        hint.setText(I18n.t("Сведите или разведите пальцы для масштаба. Перетаскивайте рамку и её углы."));
        hint.setTextSize(12); hint.setTextColor(activity.getColor(R.color.valanium_muted));
        root.addView(hint);
        LinearLayout zoomControl = (LinearLayout) slider(I18n.t("Масштаб"), 100, 500, 100, value -> view.zoomTo(value / 100f, view.getWidth()/2f, view.getHeight()/2f));
        zoomBar = (SeekBar) zoomControl.getChildAt(1);
        zoomLabel = (TextView) zoomControl.getChildAt(0);
        root.addView(zoomControl);
        if (ratio == 0) root.addView(ratioRow());
        root.addView(toolRow());
        root.addView(slider(I18n.t("Яркость"), 50, 160, brightness, value -> {
            brightness = value;
            view.invalidate();
            updateEstimate();
        }));
        root.addView(slider(I18n.t("Контраст"), 50, 160, contrast, value -> {
            contrast = value;
            view.invalidate();
            updateEstimate();
        }));
        root.addView(slider(I18n.t("Насыщенность"), 0, 200, saturation, value -> {
            saturation = value;
            view.invalidate();
            updateEstimate();
        }));
        root.addView(slider(I18n.t("Качество"), 40, 95, quality, value -> {
            quality = value;
            updateEstimate();
        }));

        size = new TextView(activity);
        size.setTextColor(Color.GRAY);
        size.setTextSize(10);
        size.setPadding(0, dp(6), 0, 0);
        root.addView(size);

        ScrollView scroll = new ScrollView(activity);
        scroll.setFillViewport(false); scroll.addView(root);
        android.content.SharedPreferences appearance=activity.getSharedPreferences("appearance",0);
        ThemeSurfaces.apply(root,appearance.getInt("accent_color",0xff9770ff),"light".equals(appearance.getString("theme","dark")));
        AlertDialog dialog = new AlertDialog.Builder(activity)
                .setTitle(ratio == 1 ? I18n.t("Аватар") : I18n.t("Фото"))
                .setView(scroll)
                .setPositiveButton(ratio == 1 ? I18n.t("Сохранить") : I18n.t("Отправить"), null)
                .setNegativeButton(I18n.t("Отмена"), null)
                .create();

        dialog.setOnShowListener(d -> {
            send = dialog.getButton(AlertDialog.BUTTON_POSITIVE);
            send.setOnClickListener(v -> {
                Rendered rendered = render();
                if (rendered == null) return;
                dialog.dismiss();
                result.ready(rendered.base64, rendered.width, rendered.height);
            });
            updateEstimate();
        });
        dialog.show();
    }

    // --- сборка изображения ---------------------------------------------------

    /** Пересобирает {@link #work} с учётом поворота и отражения. */
    private void rebuildWork() {
        Matrix matrix = new Matrix();
        matrix.postRotate(rotation);
        if (flipped) matrix.postScale(-1, 1);

        Bitmap turned = Bitmap.createBitmap(source, 0, 0, source.getWidth(), source.getHeight(), matrix, true);
        float scale = Math.min(1f, (float) WORK_MAX_SIDE / Math.max(turned.getWidth(), turned.getHeight()));
        if (scale < 1f) {
            int width = Math.max(1, Math.round(turned.getWidth() * scale));
            int height = Math.max(1, Math.round(turned.getHeight() * scale));
            Bitmap scaled = Bitmap.createScaledBitmap(turned, width, height, true);
            if (scaled != turned && turned != source) turned.recycle();
            turned = scaled;
        }
        if (work != null && work != source) work.recycle();
        work = turned;
    }

    /** Кадр по умолчанию: всё изображение, подрезанное под соотношение. */
    private void resetCrop() {
        if (view != null) view.resetViewport();
        float width = work.getWidth();
        float height = work.getHeight();
        if (ratio == 0) {
            crop = new RectF(0, 0, width, height);
            return;
        }
        float cropWidth = width;
        float cropHeight = width / ratio;
        if (cropHeight > height) {
            cropHeight = height;
            cropWidth = height * ratio;
        }
        float left = (width - cropWidth) / 2;
        float top = (height - cropHeight) / 2;
        crop = new RectF(left, top, left + cropWidth, top + cropHeight);
    }

    private ColorMatrixColorFilter filter() {
        ColorMatrix matrix = new ColorMatrix();
        matrix.setSaturation(saturation / 100f);

        // Яркость — сдвиг, контраст — масштаб вокруг середины диапазона.
        float c = contrast / 100f;
        float b = (brightness - 100) * 255f / 100f;
        float shift = 127.5f * (1 - c) + b;
        ColorMatrix tone = new ColorMatrix(new float[]{
                c, 0, 0, 0, shift,
                0, c, 0, 0, shift,
                0, 0, c, 0, shift,
                0, 0, 0, 1, 0,
        });
        matrix.postConcat(tone);
        return new ColorMatrixColorFilter(matrix);
    }

    private static final class Rendered {
        final String base64;
        final int width;
        final int height;

        Rendered(String base64, int width, int height) {
            this.base64 = base64;
            this.width = width;
            this.height = height;
        }
    }

    /** Итоговый JPEG. Качество снижается, пока результат не влезет в лимит. */
    private Rendered render() {
        int width = Math.max(1, Math.round(crop.width()));
        int height = Math.max(1, Math.round(crop.height()));

        Bitmap out = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
        Canvas canvas = new Canvas(out);
        canvas.drawColor(Color.BLACK);
        Paint paint = new Paint(Paint.FILTER_BITMAP_FLAG);
        paint.setColorFilter(filter());
        Rect src = new Rect(Math.round(crop.left), Math.round(crop.top),
                Math.round(crop.right), Math.round(crop.bottom));
        canvas.drawBitmap(work, src, new Rect(0, 0, width, height), paint);

        int attempt = quality;
        String encoded = null;
        while (attempt >= 40) {
            ByteArrayOutputStream buffer = new ByteArrayOutputStream();
            out.compress(Bitmap.CompressFormat.JPEG, attempt, buffer);
            encoded = Base64.encodeToString(buffer.toByteArray(), Base64.NO_WRAP);
            if (encoded.length() <= maxBase64) break;
            attempt -= 10;
        }
        out.recycle();

        if (encoded == null || encoded.length() > maxBase64) return null;
        return new Rendered(encoded, width, height);
    }

    /** Показывает вес результата: иначе отказ прилетает уже при отправке. */
    private final Runnable estimateTask=this::estimateNow;
    private void updateEstimate() {
        if(view==null)return;view.removeCallbacks(estimateTask);view.postDelayed(estimateTask,120);
    }
    private void estimateNow() {
        Rendered rendered = render();
        if (rendered == null) {
            size.setText(I18n.t("не помещается — уменьшите кадр"));
            if (send != null) send.setEnabled(false);
            return;
        }
        int kb = Math.round(rendered.base64.length() * 0.75f / 1024);
        size.setText(rendered.width + "×" + rendered.height + " · " + kb + I18n.t(" КБ"));
        if (send != null) send.setEnabled(true);
    }

    // --- органы управления ------------------------------------------------------

    private View ratioRow() {
        LinearLayout row = new LinearLayout(activity);
        row.setOrientation(LinearLayout.HORIZONTAL);
        LinearLayout.LayoutParams params =
                new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(38));
        params.topMargin = dp(9);
        row.setLayoutParams(params);

        String[][] options = {{I18n.t("Свободно"), "0"}, {"1:1", "1"}, {"4:3", "1.3333"}, {"16:9", "1.7778"}};
        for (String[] option : options) {
            Button button = new Button(activity, null, 0, R.style.Valanium_Button_Dark_Small);
            button.setText(option[0]);
            button.setTextSize(10);
            LinearLayout.LayoutParams buttonParams =
                    new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f);
            if (row.getChildCount() > 0) buttonParams.leftMargin = dp(4);
            button.setLayoutParams(buttonParams);
            button.setOnClickListener(v -> {
                ratio = Float.parseFloat(option[1]);
                resetCrop();
                view.invalidate();
                updateEstimate();
            });
            row.addView(button);
        }
        return row;
    }

    private View toolRow() {
        LinearLayout row = new LinearLayout(activity);
        row.setOrientation(LinearLayout.HORIZONTAL);
        LinearLayout.LayoutParams params =
                new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(38));
        params.topMargin = dp(5);
        row.setLayoutParams(params);

        row.addView(tool(I18n.t("Повернуть"), () -> {
            rotation = (rotation + 90) % 360;
            rebuildWork();
            resetCrop();
            view.invalidate();
            updateEstimate();
        }, 0));
        row.addView(tool(I18n.t("Отразить"), () -> {
            flipped = !flipped;
            rebuildWork();
            view.invalidate();
            updateEstimate();
        }, dp(4)));
        row.addView(tool(I18n.t("Сбросить"), () -> {
            rotation = 0;
            flipped = false;
            brightness = contrast = saturation = 100;
            rebuildWork();
            resetCrop();
            view.invalidate();
            updateEstimate();
        }, dp(4)));
        return row;
    }

    private Button tool(String caption, Runnable action, int leftMargin) {
        Button button = new Button(activity, null, 0, R.style.Valanium_Button_Dark_Small);
        button.setText(caption);
        button.setTextSize(10);
        LinearLayout.LayoutParams params =
                new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f);
        params.leftMargin = leftMargin;
        button.setLayoutParams(params);
        button.setOnClickListener(v -> action.run());
        return button;
    }

    private interface OnValue {
        void changed(int value);
    }

    private View slider(String caption, int min, int max, int value, OnValue listener) {
        LinearLayout row = new LinearLayout(activity);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);

        TextView label = new TextView(activity);
        label.setText(caption);
        label.setTextColor(Color.GRAY);
        label.setTextSize(10);
        label.setLayoutParams(new LinearLayout.LayoutParams(dp(96),
                LinearLayout.LayoutParams.WRAP_CONTENT));
        row.addView(label);

        SeekBar bar = new SeekBar(activity);
        bar.setMax(max - min);
        bar.setProgress(value - min);
        bar.setLayoutParams(new LinearLayout.LayoutParams(0, dp(34), 1f));
        bar.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
            @Override
            public void onProgressChanged(SeekBar seekBar, int progress, boolean fromUser) {
                if (fromUser) listener.changed(min + progress);
            }

            @Override
            public void onStartTrackingTouch(SeekBar seekBar) {
            }

            @Override
            public void onStopTrackingTouch(SeekBar seekBar) {
            }
        });
        row.addView(bar);
        return row;
    }

    private int dp(int value) {
        return Math.round(value * activity.getResources().getDisplayMetrics().density);
    }

    // --- холст с рамкой кадра -----------------------------------------------------

    /**
     * Показывает изображение вписанным и даёт таскать рамку.
     *
     * Масштаб считается явно, а не отдаётся на откуп ImageView: рамка живёт в
     * координатах изображения, и без известного масштаба её нечем переводить в
     * пиксели экрана.
     */
    private final class CropView extends View {

        private final Paint framePaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint shadePaint = new Paint();
        private final Paint imagePaint = new Paint(Paint.FILTER_BITMAP_FLAG);

        /** Что тянем: null — всю рамку, иначе угол. */
        private String handle;
        private float startX;
        private float startY;
        private RectF startCrop;
        private float zoom=1f, panX, panY, startPanX, startPanY;
        private boolean transformed;
        private final ScaleGestureDetector scaler;
        private final GestureDetector gestures;
        private final Runnable estimate = () -> updateEstimate();

        CropView(Activity context) {
            super(context);
            framePaint.setStyle(Paint.Style.STROKE);
            framePaint.setStrokeWidth(dp(2));
            framePaint.setColor(Color.WHITE);
            shadePaint.setColor(Color.argb(140, 0, 0, 0));
            scaler = new ScaleGestureDetector(context, new ScaleGestureDetector.SimpleOnScaleGestureListener() {
                @Override public boolean onScale(ScaleGestureDetector detector) {
                    transformed=true;zoomTo(zoom*detector.getScaleFactor(),detector.getFocusX(),detector.getFocusY());return true;
                }
            });
            gestures = new GestureDetector(context,new GestureDetector.SimpleOnGestureListener(){
                @Override public boolean onDown(MotionEvent event){return true;}
                @Override public boolean onDoubleTap(MotionEvent event){transformed=true;zoomTo(zoom>1.1f?1f:2f,event.getX(),event.getY());return true;}
            });
        }

        private float scale() {
            return zoom * Math.min((float) getWidth() / work.getWidth(), (float) getHeight() / work.getHeight());
        }

        private float offsetX() {
            return (getWidth() - work.getWidth() * scale()) / 2 + panX;
        }

        private float offsetY() {
            return (getHeight() - work.getHeight() * scale()) / 2 + panY;
        }

        void resetViewport() { zoom=1f;panX=panY=0; if(zoomBar!=null)zoomBar.setProgress(0); }
        void zoomTo(float wanted,float focusX,float focusY) {
            if(getWidth()==0||getHeight()==0)return;
            RectF frame=cropOnScreen();float before=scale();
            float anchorX=(focusX-offsetX())/before,anchorY=(focusY-offsetY())/before;
            float base=before/zoom;
            float minimum=Math.min(MIN_CROP,Math.min(work.getWidth(),work.getHeight()));
            float limit=Math.max(1f,Math.min(5f,Math.min(frame.width(),frame.height())/(base*minimum)));
            zoom=Math.max(1f,Math.min(limit,wanted));float after=scale();
            panX=focusX-anchorX*after-(getWidth()-work.getWidth()*after)/2;
            panY=focusY-anchorY*after-(getHeight()-work.getHeight()*after)/2;
            // Keep the picture covering the same frame even when the pinch
            // focus is close to an edge. Clamping only the crop made it jump.
            float originX=clamp(offsetX(),frame.right-work.getWidth()*after,frame.left);
            float originY=clamp(offsetY(),frame.bottom-work.getHeight()*after,frame.top);
            panX=originX-(getWidth()-work.getWidth()*after)/2;
            panY=originY-(getHeight()-work.getHeight()*after)/2;
            float width=Math.min(work.getWidth(),Math.max(MIN_CROP,frame.width()/after));
            float height=Math.min(work.getHeight(),Math.max(MIN_CROP,frame.height()/after));
            if(ratio!=0){width=Math.min(width,height*ratio);height=width/ratio;}
            float left=clamp((frame.left-offsetX())/after,0,work.getWidth()-width);
            float top=clamp((frame.top-offsetY())/after,0,work.getHeight()-height);
            crop.set(left,top,left+width,top+height);
            if(zoomBar!=null){zoomBar.setProgress(Math.round((zoom-1)*100));zoomBar.setContentDescription(I18n.t("Масштаб")+" "+String.format(java.util.Locale.getDefault(),"%.1f×",zoom));}
            if(zoomLabel!=null)zoomLabel.setText(I18n.t("Масштаб")+" "+String.format(java.util.Locale.getDefault(),"%.1f×",zoom));
            invalidate();removeCallbacks(estimate);postDelayed(estimate,150);
        }

        @Override protected void onDetachedFromWindow(){removeCallbacks(estimate);super.onDetachedFromWindow();}

        private RectF cropOnScreen() {
            float s = scale();
            return new RectF(
                    offsetX() + crop.left * s, offsetY() + crop.top * s,
                    offsetX() + crop.right * s, offsetY() + crop.bottom * s);
        }

        @Override
        protected void onDraw(Canvas canvas) {
            if (work == null) return;
            float s = scale();
            RectF target = new RectF(offsetX(), offsetY(),
                    offsetX() + work.getWidth() * s, offsetY() + work.getHeight() * s);
            imagePaint.setColorFilter(filter());
            canvas.drawBitmap(work, null, target, imagePaint);

            // Затемнение вне рамки — четырьмя полосами: без него не видно, что
            // именно уйдёт в сообщение.
            RectF box = cropOnScreen();
            canvas.drawRect(target.left, target.top, target.right, box.top, shadePaint);
            canvas.drawRect(target.left, box.bottom, target.right, target.bottom, shadePaint);
            canvas.drawRect(target.left, box.top, box.left, box.bottom, shadePaint);
            canvas.drawRect(box.right, box.top, target.right, box.bottom, shadePaint);
            canvas.drawRect(box, framePaint);
            framePaint.setStrokeWidth(dp(1));framePaint.setAlpha(100);
            for(int i=1;i<3;i++){float x=box.left+box.width()*i/3,y=box.top+box.height()*i/3;canvas.drawLine(x,box.top,x,box.bottom,framePaint);canvas.drawLine(box.left,y,box.right,y,framePaint);}
            framePaint.setStrokeWidth(dp(2));framePaint.setAlpha(255);
        }

        @Override
        public boolean onTouchEvent(MotionEvent event) {
            if(event.getActionMasked()==MotionEvent.ACTION_DOWN)transformed=false;
            scaler.onTouchEvent(event); gestures.onTouchEvent(event);
            if(scaler.isInProgress()||event.getPointerCount()>1||transformed){
                getParent().requestDisallowInterceptTouchEvent(true);
                if(event.getActionMasked()==MotionEvent.ACTION_UP)updateEstimate();
                return true;
            }
            float s = scale();
            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN: {
                    RectF box = cropOnScreen();
                    float grab = dp(28);
                    handle = null;
                    if (near(event, box.left, box.top, grab)) handle = "nw";
                    else if (near(event, box.right, box.top, grab)) handle = "ne";
                    else if (near(event, box.left, box.bottom, grab)) handle = "sw";
                    else if (near(event, box.right, box.bottom, grab)) handle = "se";
                    startX = event.getX();
                    startY = event.getY();
                    startCrop = new RectF(crop);
                    startPanX=panX;startPanY=panY;
                    getParent().requestDisallowInterceptTouchEvent(true);
                    return true;
                }
                case MotionEvent.ACTION_MOVE: {
                    float dx = (event.getX() - startX) / s;
                    float dy = (event.getY() - startY) / s;
                    if(handle==null&&zoom>1.01f){crop=moved(-dx,-dy);panX=startPanX-(crop.left-startCrop.left)*s;panY=startPanY-(crop.top-startCrop.top)*s;}
                    else crop = handle == null ? moved(dx, dy) : resized(dx, dy);
                    invalidate();
                    return true;
                }
                case MotionEvent.ACTION_UP:
                case MotionEvent.ACTION_CANCEL:
                    updateEstimate();
                    return true;
                default:
                    return super.onTouchEvent(event);
            }
        }

        private boolean near(MotionEvent event, float x, float y, float grab) {
            return Math.abs(event.getX() - x) < grab && Math.abs(event.getY() - y) < grab;
        }

        private RectF moved(float dx, float dy) {
            float width = startCrop.width();
            float height = startCrop.height();
            float left = clamp(startCrop.left + dx, 0, work.getWidth() - width);
            float top = clamp(startCrop.top + dy, 0, work.getHeight() - height);
            return new RectF(left, top, left + width, top + height);
        }

        private RectF resized(float dx, float dy) {
            boolean west=handle.contains("w"),north=handle.contains("n");
            float anchorX=west?startCrop.right:startCrop.left,anchorY=north?startCrop.bottom:startCrop.top;
            float maxWidth=west?anchorX:work.getWidth()-anchorX,maxHeight=north?anchorY:work.getHeight()-anchorY;
            float minimum=Math.min(MIN_CROP,Math.min(work.getWidth(),work.getHeight()));
            float width=clamp(startCrop.width()+(west?-dx:dx),Math.min(minimum,maxWidth),maxWidth);
            float height=clamp(startCrop.height()+(north?-dy:dy),Math.min(minimum,maxHeight),maxHeight);
            if(ratio!=0){width=Math.min(width,maxHeight*ratio);height=width/ratio;}
            float left=west?anchorX-width:anchorX,top=north?anchorY-height:anchorY;
            return new RectF(left,top,left+width,top+height);
        }

        private float clamp(float value, float min, float max) {
            return Math.min(Math.max(value, min), Math.max(min, max));
        }
    }
}
