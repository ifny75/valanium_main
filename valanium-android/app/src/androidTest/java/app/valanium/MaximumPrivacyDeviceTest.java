package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.test.InstrumentationTestCase;
import android.view.View;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import org.json.JSONObject;

/** Applies the real core command via the UI, only in the isolated QA package. */
public final class MaximumPrivacyDeviceTest extends InstrumentationTestCase {
    private void call(Activity activity, String name, Class<?>[] types, Object... args) {
        try {
            Method method = MainActivity.class.getDeclaredMethod(name, types);
            method.setAccessible(true);
            method.invoke(activity, args);
        } catch (Exception error) { throw new AssertionError(error); }
    }

    private String scope(Activity activity, String name) {
        String[] result = {""};
        getInstrumentation().runOnMainSync(() -> {
            try {
                Field field = MainActivity.class.getDeclaredField("privacy");
                field.setAccessible(true);
                JSONObject privacy = (JSONObject) field.get(activity);
                if (privacy != null && privacy.optJSONObject(name) != null)
                    result[0] = privacy.optJSONObject(name).optString("scope");
            } catch (Exception error) { throw new AssertionError(error); }
        });
        return result[0];
    }

    public void testButtonAppliesToAnExistingDocumentAndSelectsOnion() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("Refusing real account", "app.valanium.qa", context.getPackageName());
        context.getSharedPreferences("appearance", 0).edit().putString("transport", "basic")
                .putBoolean("onion_default_v1", true).putBoolean("screen_privacy", false).commit();
        Activity activity = getInstrumentation().startActivitySync(
                new Intent(context, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK));
        long deadline = System.currentTimeMillis() + 15000;
        while (!ValaniumService.core().isOpen() && System.currentTimeMillis() < deadline) Thread.sleep(100);
        assertTrue(ValaniumService.core().isOpen());
        assertTrue(ValaniumService.core().submit("{\"type\":\"privacy_set\",\"privacy\":{\"presence\":{\"scope\":\"everyone\",\"allow\":[],\"deny\":[]}}}"));
        deadline = System.currentTimeMillis() + 5000;
        while (!"everyone".equals(scope(activity, "presence")) && System.currentTimeMillis() < deadline) Thread.sleep(50);
        assertEquals("everyone", scope(activity, "presence"));
        getInstrumentation().runOnMainSync(() -> {
            call(activity, "renderPrivacySections", new Class<?>[]{});
            call(activity, "show", new Class<?>[]{View.class}, activity.findViewById(R.id.screen_privacy));
            activity.findViewById(R.id.privacy_maximize).performClick();
        });
        deadline = System.currentTimeMillis() + 5000;
        while (!"nobody".equals(scope(activity, "discoverable")) && System.currentTimeMillis() < deadline) Thread.sleep(50);
        for (String name : new String[]{"presence", "typing", "read_receipts", "discoverable", "link_previews"})
            assertEquals(name, "nobody", scope(activity, name));
        assertEquals("approved", scope(activity, "direct_messages"));
        assertEquals("onion", context.getSharedPreferences("appearance", 0).getString("transport", ""));
        Thread.sleep(300);
        android.graphics.Bitmap image = getInstrumentation().getUiAutomation().takeScreenshot();
        assertNotNull(image);
        try (java.io.FileOutputStream stream = new java.io.FileOutputStream(
                new java.io.File(context.getExternalFilesDir(null), "maximum-privacy.png"))) {
            image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, stream);
        }
        getInstrumentation().runOnMainSync(activity::finish);
    }
}
