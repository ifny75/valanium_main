package app.valanium;

import android.content.Context;
import android.graphics.Color;
import android.graphics.drawable.GradientDrawable;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.TextView;
import java.util.List;

/** Full-row hit area; messages swipe toward the center without leaving the row. */
final class SwipeMessageLayout extends FrameLayout {
    static final class Action {
        final String label, icon;
        final Runnable run;
        Action(String label, String icon, Runnable run) {
            this.label = label; this.icon = icon; this.run = run;
        }
    }
    private final View bubble;
    private final TextView indicator;
    private final TextView previous;
    private final List<Action> actions;
    private MessageSwipe gesture;
    private float startX, startY;
    private int displayed = -1;
    private boolean ignored;
    private final float density;
    private final boolean outgoing;

    SwipeMessageLayout(Context context, View bubble, List<Action> actions, int accent, boolean outgoing) {
        super(context);
        this.bubble = bubble; this.actions = actions; this.outgoing = outgoing;
        density = getResources().getDisplayMetrics().density;
        indicator = new TextView(context);
        indicator.setGravity(Gravity.CENTER);
        indicator.setTextColor(Color.WHITE);
        indicator.setTextSize(20);
        GradientDrawable circle = new GradientDrawable();
        circle.setShape(GradientDrawable.OVAL); circle.setColor(accent);
        indicator.setBackground(circle);
        indicator.setAlpha(0);
        indicator.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
        LayoutParams iconParams = new LayoutParams(dp(44), dp(44), (outgoing ? Gravity.END : Gravity.START) | Gravity.CENTER_VERTICAL);
        if (outgoing) iconParams.rightMargin = dp(6);
        else iconParams.leftMargin = dp(6);
        addView(indicator, iconParams);
        previous = new TextView(context);
        previous.setGravity(Gravity.CENTER); previous.setTextColor(Color.WHITE); previous.setTextSize(20);
        previous.setBackground(circle.getConstantState().newDrawable().mutate());
        previous.setAlpha(0); previous.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
        addView(previous, new LayoutParams(iconParams));
        LayoutParams bubbleParams = new LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT, outgoing ? Gravity.END : Gravity.START);
        bubbleParams.leftMargin = outgoing ? dp(48) : 0;
        bubbleParams.rightMargin = outgoing ? 0 : dp(48);
        addView(bubble, bubbleParams);
        setClipChildren(true);
    }

    private int dp(float value) { return Math.round(value * density); }

    @Override public boolean onInterceptTouchEvent(MotionEvent event) {
        if (event.getActionMasked() == MotionEvent.ACTION_DOWN) {
            bubble.animate().cancel(); bubble.setTranslationX(0);
            indicator.animate().cancel(); indicator.setAlpha(0);
            previous.animate().cancel(); previous.setAlpha(0);
            displayed = -1; ignored = false;
            startX = event.getX(); startY = event.getY();
            // Use row coordinates, not the width of the bubble or screen rawX.
            // Incoming short messages have room to their right; outgoing to their left.
            float room = outgoing ? startX : getWidth() - startX;
            float maximum = Math.max(44, Math.min(240, room / density - 8));
            gesture = new MessageSwipe(actions.size(), maximum, outgoing ? -1 : 1);
            if (bubble.getWidth() > 0 && (startX < bubble.getLeft() || startX > bubble.getRight()
                    || startY < bubble.getTop() || startY > bubble.getBottom())) ignored = true;
        } else if (event.getPointerCount() > 1) {
            cancelGesture(); ignored = true;
        } else if (!ignored && gesture != null && event.getActionMasked() == MotionEvent.ACTION_MOVE
                && gesture.move((event.getX() - startX) / density, (event.getY() - startY) / density)) {
            getParent().requestDisallowInterceptTouchEvent(true);
            renderSelection();
            return true;
        }
        return false;
    }

    @Override public boolean onTouchEvent(MotionEvent event) {
        if (gesture == null || ignored) return false;
        if (event.getPointerCount() > 1) { cancelGesture(); ignored = true; return true; }
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_MOVE:
                gesture.move((event.getX() - startX) / density, (event.getY() - startY) / density);
                renderSelection();
                return true;
            case MotionEvent.ACTION_UP:
                gesture.move((event.getX() - startX) / density, (event.getY() - startY) / density);
                int selected = gesture.release(false);
                resetVisuals();
                if (selected >= 0) {
                    performHapticFeedback(android.view.HapticFeedbackConstants.CLOCK_TICK);
                    actions.get(selected).run.run();
                }
                return true;
            case MotionEvent.ACTION_CANCEL:
                cancelGesture(); return true;
            default: return true;
        }
    }

    private void renderSelection() {
        bubble.setTranslationX(MessageSwipe.boundedOffset(gesture.offset * density,
                bubble.getLeft(), bubble.getRight(), getWidth()));
        int next = gesture.active;
        if (next == displayed) return;
        previous.animate().cancel(); previous.setAlpha(0);
        if (displayed >= 0) {
            previous.setText(actions.get(displayed).icon);
            previous.setTranslationY(0); previous.setAlpha(indicator.getAlpha());
            previous.animate().translationY(-dp(16)).alpha(0).setDuration(120).start();
        }
        displayed = next;
        indicator.animate().cancel();
        if (next < 0) { indicator.setAlpha(0); return; }
        indicator.setText(actions.get(next).icon);
        indicator.setContentDescription(actions.get(next).label);
        indicator.setTranslationY(dp(14)); indicator.setAlpha(0);
        indicator.animate().translationY(0).alpha(1).setDuration(120).start();
    }

    private void cancelGesture() {
        if (gesture != null) gesture.release(true);
        resetVisuals();
    }

    private void resetVisuals() {
        if (getParent() != null) getParent().requestDisallowInterceptTouchEvent(false);
        bubble.animate().translationX(0).setDuration(180)
                .setInterpolator(new android.view.animation.DecelerateInterpolator()).start();
        indicator.animate().alpha(0).translationY(-dp(12)).setDuration(120).start();
        previous.animate().alpha(0).setDuration(100).start();
        if (gesture != null) gesture.reset();
        displayed = -1;
    }

    @Override protected void onDetachedFromWindow() {
        bubble.animate().cancel(); bubble.setTranslationX(0);
        indicator.animate().cancel(); indicator.setAlpha(0);
        previous.animate().cancel(); previous.setAlpha(0);
        if (gesture != null) gesture.reset();
        super.onDetachedFromWindow();
    }
}
