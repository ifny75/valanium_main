package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.test.InstrumentationTestCase;
import android.view.View;
import android.view.ViewGroup;
import android.widget.EditText;
import android.widget.ScrollView;
import android.widget.TextView;
import java.lang.reflect.Method;

/** Isolated UI checks: never writes to the user's installed account. */
public final class ComposerAndScrollDeviceTest extends InstrumentationTestCase {
    private void show(Activity activity, int id) {
        try {
            Method method = MainActivity.class.getDeclaredMethod("show", View.class);
            method.setAccessible(true);
            method.invoke(activity, activity.findViewById(id));
        } catch (Exception error) { throw new AssertionError(error); }
    }

    private ScrollView scroll(View view) {
        if (view instanceof ScrollView) return (ScrollView) view;
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                ScrollView found = scroll(group.getChildAt(i));
                if (found != null) return found;
            }
        }
        return null;
    }

    public void testInlineCalculationAndSettingsScroll() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("Refusing real account", "app.valanium.qa", context.getPackageName());
        context.getSharedPreferences("appearance", 0).edit()
                .putBoolean("screen_privacy", false).commit();
        Activity activity = getInstrumentation().startActivitySync(
                new Intent(context, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        Thread.sleep(1800);
        getInstrumentation().runOnMainSync(() -> {
            show(activity, R.id.screen_conversation);
            EditText composer = activity.findViewById(R.id.composer);
            TextView result = activity.findViewById(R.id.composer_calculation);
            composer.setText("max(sqrt(16);5)");
            assertEquals("=5", result.getText().toString());
            assertEquals(View.VISIBLE, result.getVisibility());
            composer.setText("2+3=");
            assertEquals("5", result.getText().toString());
            composer.setText("привет");
            assertEquals(View.GONE, result.getVisibility());
            composer.setText("2*(3+4)");
        });
        Thread.sleep(400);
        android.graphics.Bitmap composerShot = getInstrumentation().getUiAutomation().takeScreenshot();
        assertNotNull(composerShot);
        try (java.io.FileOutputStream stream = new java.io.FileOutputStream(
                new java.io.File(context.getExternalFilesDir(null), "composer-calculation.png"))) {
            composerShot.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, stream);
        }
        getInstrumentation().runOnMainSync(() -> {
            ((EditText) activity.findViewById(R.id.composer)).setText("");
            show(activity, R.id.screen_settings);
        });
        Thread.sleep(400);
        for (int y : new int[]{400, 800, 0, 600, 160}) {
            getInstrumentation().runOnMainSync(() -> {
                ScrollView settings = scroll(activity.findViewById(R.id.screen_settings));
                assertNotNull(settings);
                assertEquals("Viewport clips cards below the fixed header",
                        new android.graphics.Rect(0, 0, settings.getWidth(), settings.getHeight()),
                        settings.getClipBounds());
                ViewGroup host = (ViewGroup) settings.getParent();
                assertTrue("Header draws above the scrolling content",
                        host.getChildAt(0).getZ() > settings.getZ());
                settings.scrollTo(0, y);
            });
            Thread.sleep(250);
        }
        android.graphics.Bitmap screenshot = getInstrumentation().getUiAutomation().takeScreenshot();
        assertNotNull(screenshot);
        java.io.File output = new java.io.File(context.getExternalFilesDir(null), "settings-scroll.png");
        try (java.io.FileOutputStream stream = new java.io.FileOutputStream(output)) {
            screenshot.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, stream);
        }
        getInstrumentation().runOnMainSync(activity::finish);
    }
}
