package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Color;
import android.test.InstrumentationTestCase;
import android.view.View;
import android.view.ViewGroup;
import java.lang.reflect.Method;

public final class SettingsDeviceTest extends InstrumentationTestCase {
    private View topBar(View view) {
        if ("valanium_top_bar".equals(view.getTag())) return view;
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                View found = topBar(group.getChildAt(i));
                if (found != null) return found;
            }
        }
        return null;
    }

    private void assertFullWidthTopBar(Activity activity, View screen) {
        View root = activity.findViewById(R.id.app_root);
        View header = topBar(screen);
        assertNotNull("Screen has a top bar", header);
        assertEquals("Top bar compensates the content inset", -root.getPaddingLeft(),
                header.getTranslationX(), .5f);
        assertEquals("Top bar is laid out for the full display width", root.getWidth(),
                header.getLayoutParams().width);
        assertEquals("Screen transition never moves the top bar", 0f,
                screen.getTranslationX(), .1f);
        assertEquals("Screen transition never fades the top bar", 1f,
                header.getAlpha(), .01f);
    }

    private Object call(Activity activity, String name, Class<?>[] types, Object... args) {
        try {
            Method method = MainActivity.class.getDeclaredMethod(name, types);
            method.setAccessible(true);
            return method.invoke(activity, args);
        } catch (Exception error) { throw new AssertionError(error); }
    }

    public void testSectionsAccentAndSafeReset() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("Refusing real account", "app.valanium.qa", context.getPackageName());
        context.getSharedPreferences("appearance", 0).edit().remove("accent_color").commit();
        Activity activity = getInstrumentation().startActivitySync(
                new Intent(context, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        Thread.sleep(1800);
        getInstrumentation().runOnMainSync(() -> {
            assertEquals(Color.rgb(151, 112, 255), call(activity, "accentColor", new Class<?>[]{}));
            call(activity, "show", new Class<?>[]{View.class}, activity.findViewById(R.id.screen_entry));
            activity.findViewById(R.id.entry_submit).setEnabled(false);
            org.json.JSONObject failure = new org.json.JSONObject();
            try { failure.put("code", "transport").put("message", "Test connection failure"); }
            catch (org.json.JSONException error) { throw new AssertionError(error); }
            call(activity, "onFailed", new Class<?>[]{org.json.JSONObject.class}, failure);
            assertTrue("Registration can retry after network failure",
                    activity.findViewById(R.id.entry_submit).isEnabled());
            call(activity, "switchTab", new Class<?>[]{View.class}, activity.findViewById(R.id.screen_settings));
        });
        Thread.sleep(400);
        getInstrumentation().runOnMainSync(() -> {
            View settings = activity.findViewById(R.id.screen_settings);
            View bar = activity.findViewById(R.id.tab_bar);
            View selectedNavigation = activity.findViewById(R.id.nav_settings);
            View selectedIcon = activity.findViewById(R.id.nav_settings_icon);
            assertFullWidthTopBar(activity, settings);
            assertEquals("Root settings do not show a dead back action", View.INVISIBLE,
                    activity.findViewById(R.id.settings_back).getVisibility());
            assertNotNull("Active navigation has a soft selection surface",
                    selectedNavigation.getBackground());
            assertEquals("Navigation motion settles at natural scale",
                    1f, selectedIcon.getScaleX(), .02f);
            assertEquals("Navigation motion settles without leaving rotation",
                    0f, selectedIcon.getRotation(), .5f);
            assertTrue("Scroll can lift last rows above glass navigation",
                    settings.getPaddingBottom() >= bar.getHeight());
            assertTrue(((android.widget.Switch) activity.findViewById(R.id.entry_tor_only)).isChecked());
            activity.findViewById(R.id.open_connection).performClick();
            assertEquals(View.VISIBLE, activity.findViewById(R.id.screen_connection).getVisibility());
            assertFullWidthTopBar(activity, activity.findViewById(R.id.screen_connection));
            assertEquals(View.GONE, bar.getVisibility());
            activity.findViewById(R.id.connection_back).performClick();
            assertEquals(View.VISIBLE, settings.getVisibility());
            activity.findViewById(R.id.open_protection).performClick();
            assertEquals(View.VISIBLE, activity.findViewById(R.id.screen_protection).getVisibility());
            assertTrue(activity.findViewById(R.id.hide_from_screenshots).isShown());
            activity.findViewById(R.id.protection_back).performClick();
            call(activity, "setAccent", new Class<?>[]{int.class}, Color.BLUE);
            assertEquals(Color.BLUE, context.getSharedPreferences("appearance", 0).getInt("accent_color", 0));
            // No preference writes to security or transport from appearance reset.
            context.getSharedPreferences("appearance", 0).edit().putBoolean("screen_privacy", false)
                    .putString("transport", "onion").commit();
            call(activity, "resetAppearance", new Class<?>[]{});
            assertFalse(context.getSharedPreferences("appearance", 0).getBoolean("screen_privacy", true));
            assertEquals("onion", context.getSharedPreferences("appearance", 0).getString("transport", ""));
            assertEquals(Color.rgb(151, 112, 255), call(activity, "accentColor", new Class<?>[]{}));
            activity.finish();
        });
    }
}
