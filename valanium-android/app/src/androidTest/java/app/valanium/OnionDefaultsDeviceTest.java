package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.test.InstrumentationTestCase;
import android.widget.Switch;
import java.lang.reflect.Method;

public final class OnionDefaultsDeviceTest extends InstrumentationTestCase {
    private void checkRoute(String previous, String expected) throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("Refusing real account", "app.valanium.qa", context.getPackageName());
        context.getSharedPreferences("appearance", 0).edit()
                .putString("transport", previous).remove("onion_default_v1").commit();
        Activity activity = getInstrumentation().startActivitySync(
                new Intent(context, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK));
        Thread.sleep(700);
        getInstrumentation().runOnMainSync(() -> {
            assertEquals(expected, context.getSharedPreferences("appearance", 0).getString("transport", ""));
            assertTrue(((Switch) activity.findViewById(R.id.entry_tor_only)).isChecked());
            try {
                Method url = MainActivity.class.getDeclaredMethod("serverUrl");
                url.setAccessible(true);
                assertEquals(expected.equals("onion") ? "valanium://onion" : "wss://valanium.com/ws",
                        url.invoke(activity));
                Method permits = MainActivity.class.getDeclaredMethod("permits", String.class, String.class);
                permits.setAccessible(true);
                java.lang.reflect.Field privacy = MainActivity.class.getDeclaredField("privacy");
                privacy.setAccessible(true);
                privacy.set(activity, null);
                assertEquals(false, permits.invoke(activity, "typing", "peer"));
            } catch (Exception error) { throw new AssertionError(error); }
            activity.finish();
        });
    }
    public void testFormerAutoDefaultBecomesOnion() throws Exception { checkRoute("auto", "onion"); }
    public void testExplicitRelayChoiceIsPreserved() throws Exception { checkRoute("basic", "basic"); }
}
