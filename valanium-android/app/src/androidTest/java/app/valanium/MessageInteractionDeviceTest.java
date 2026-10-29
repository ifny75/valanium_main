package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.content.res.Configuration;
import android.test.InstrumentationTestCase;
import android.view.LayoutInflater;
import android.view.MotionEvent;
import android.view.View;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.lang.reflect.Method;
import java.util.Arrays;
import java.util.concurrent.atomic.AtomicInteger;

/** Runs only under the isolated QA application id, without opening a real account. */
public final class MessageInteractionDeviceTest extends InstrumentationTestCase {
    public void testTabletEntryResourcesAndSeedOnlyForm() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("app.valanium.qa", context.getPackageName());
        getInstrumentation().runOnMainSync(() -> {
            for (int width : new int[]{400, 600, 1000}) {
                Configuration config = new Configuration(context.getResources().getConfiguration());
                config.screenWidthDp = width;
                Context sized = context.createConfigurationContext(config);
                View entry = LayoutInflater.from(sized).inflate(R.layout.entry_screen, null);
                assertNotNull(entry.findViewById(R.id.entry_submit));
                assertNotNull(entry.findViewById(R.id.open_recover));
                assertNotNull(entry.findViewById(R.id.entry_tor_only));
                assertEquals(width >= 840, entry instanceof LinearLayout);
                assertEquals("No personal credential inputs", 1, countInputs(entry));
            }
        });
    }

    private int countInputs(View view) {
        int count = view instanceof EditText ? 1 : 0;
        if (view instanceof android.view.ViewGroup) {
            android.view.ViewGroup group = (android.view.ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) count += countInputs(group.getChildAt(i));
        }
        return count;
    }

    public void testNativeSwipeExecutesOnlyOnReleaseAndCancelDoesNothing() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("app.valanium.qa", context.getPackageName());
        getInstrumentation().runOnMainSync(() -> {
            AtomicInteger calls = new AtomicInteger();
            TextView bubble = new TextView(context); bubble.setText("Test message");
            SwipeMessageLayout layout = new SwipeMessageLayout(context, bubble,
                    Arrays.asList(new SwipeMessageLayout.Action("Reply", "↩", calls::incrementAndGet)), 0xff8251dd, true);
            android.widget.FrameLayout parent = new android.widget.FrameLayout(context);
            parent.addView(layout);
            float d = context.getResources().getDisplayMetrics().density;
            touch(layout, true, MotionEvent.ACTION_DOWN, 220 * d, 40 * d);
            assertTrue(touch(layout, true, MotionEvent.ACTION_MOVE, 150 * d, 41 * d));
            assertEquals(0, calls.get());
            touch(layout, false, MotionEvent.ACTION_UP, 150 * d, 41 * d);
            assertEquals(1, calls.get());
            touch(layout, true, MotionEvent.ACTION_DOWN, 220 * d, 40 * d);
            touch(layout, true, MotionEvent.ACTION_MOVE, 150 * d, 41 * d);
            touch(layout, false, MotionEvent.ACTION_CANCEL, 150 * d, 41 * d);
            assertEquals(1, calls.get());
        });
    }

    public void testShortIncomingAndLongOutgoingStayInsideFullRow() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("app.valanium.qa", context.getPackageName());
        getInstrumentation().runOnMainSync(() -> {
            float d = context.getResources().getDisplayMetrics().density;
            for (boolean outgoing : new boolean[]{false, true}) {
                for (int bubbleWidth : new int[]{48, 300}) {
                    AtomicInteger calls = new AtomicInteger();
                    TextView bubble = new TextView(context);
                    bubble.setText("АГА"); bubble.setWidth(Math.round(bubbleWidth * d));
                    SwipeMessageLayout layout = new SwipeMessageLayout(context, bubble,
                            Arrays.asList(new SwipeMessageLayout.Action("Reply", "↩", calls::incrementAndGet)),
                            0xff8251dd, outgoing);
                    android.widget.FrameLayout parent = new android.widget.FrameLayout(context);
                    parent.addView(layout, new android.widget.FrameLayout.LayoutParams(-1, -2));
                    parent.measure(View.MeasureSpec.makeMeasureSpec(Math.round(360 * d), View.MeasureSpec.EXACTLY),
                            View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED));
                    parent.layout(0, 0, parent.getMeasuredWidth(), parent.getMeasuredHeight());
                    float x = (bubble.getLeft() + bubble.getRight()) / 2f;
                    float y = (bubble.getTop() + bubble.getBottom()) / 2f;
                    float end = x + (outgoing ? -90 : 90) * d;
                    touch(layout, true, MotionEvent.ACTION_DOWN, x, y);
                    assertTrue(touch(layout, true, MotionEvent.ACTION_MOVE, end, y));
                    assertTrue(bubble.getLeft() + bubble.getTranslationX() >= 0);
                    assertTrue(bubble.getRight() + bubble.getTranslationX() <= layout.getWidth());
                    assertEquals(0, calls.get());
                    touch(layout, false, MotionEvent.ACTION_UP, end, y);
                    assertEquals(1, calls.get());
                }
            }
        });
    }

    private boolean touch(SwipeMessageLayout layout, boolean intercept, int action, float x, float y) {
        MotionEvent event = MotionEvent.obtain(1, 2, action, x, y, 0);
        try { return intercept ? layout.onInterceptTouchEvent(event) : layout.onTouchEvent(event); }
        finally { event.recycle(); }
    }

    public void testEmptySendKeepsComposerFocused() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("app.valanium.qa", context.getPackageName());
        Activity activity = getInstrumentation().startActivitySync(new Intent(context, MainActivity.class)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        getInstrumentation().runOnMainSync(() -> {
            try {
                Method show = MainActivity.class.getDeclaredMethod("show", View.class); show.setAccessible(true);
                show.invoke(activity, activity.findViewById(R.id.screen_conversation));
                EditText composer = activity.findViewById(R.id.composer);
                composer.setText("");
                activity.findViewById(R.id.send).performClick();
                assertTrue(composer.hasFocus());
            } catch (Exception error) { throw new AssertionError(error); }
            activity.finish();
        });
    }
}
