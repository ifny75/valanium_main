package app.valanium;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Context;
import android.content.Intent;
import android.test.InstrumentationTestCase;
import java.io.File;
import java.lang.reflect.Method;

/** Run only with -PisolatedTest: the package guard is deliberate. */
public final class LogoutDeviceTest extends InstrumentationTestCase {
    public void testLogoutPreservesOldDatabaseAndReopensFreshSession() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("Refusing to touch the real account", "app.valanium.qa", context.getPackageName());
        Intent intent = new Intent(context, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        Activity activity = getInstrumentation().startActivitySync(intent);
        long deadline = System.currentTimeMillis() + 15000;
        while (!ValaniumService.core().isOpen() && System.currentTimeMillis() < deadline) Thread.sleep(100);
        assertTrue("Initial database opens", ValaniumService.core().isOpen());
        getInstrumentation().waitForIdleSync();
        getInstrumentation().runOnMainSync(() -> {
            assertNotNull(activity.findViewById(R.id.account_share));
            assertNotNull(activity.findViewById(R.id.account_reconnect));
            assertNotNull(activity.findViewById(R.id.account_updates));
            assertNotNull(activity.findViewById(R.id.account_logout));
        });
        String original = context.getSharedPreferences("account_session", 0).getString("database", "valanium.db");
        File oldDatabase = new File(context.getFilesDir(), original);
        assertTrue(oldDatabase.exists());
        Instrumentation.ActivityMonitor monitor = getInstrumentation().addMonitor(MainActivity.class.getName(), null, false);
        getInstrumentation().runOnMainSync(() -> {
            try {
                Method logout = MainActivity.class.getDeclaredMethod("logoutAccount");
                logout.setAccessible(true);
                logout.invoke(activity);
            } catch (Exception error) { throw new AssertionError(error); }
        });
        Activity next = monitor.waitForActivityWithTimeout(15000);
        getInstrumentation().removeMonitor(monitor);
        assertNotNull("Entry activity restarted after shutdown", next);
        deadline = System.currentTimeMillis() + 15000;
        while (!ValaniumService.core().isOpen() && System.currentTimeMillis() < deadline) Thread.sleep(100);
        assertTrue("New session opens after service closes old core", ValaniumService.core().isOpen());
        assertFalse(ValaniumService.isSigningOut());
        String current = context.getSharedPreferences("account_session", 0).getString("database", "valanium.db");
        assertFalse("Different active database", original.equals(current));
        assertTrue("Old encrypted database retained", oldDatabase.exists());
        assertTrue("New database created", new File(context.getFilesDir(), current).exists());
        // A delayed old onDestroy must not close the newly opened core.
        Thread.sleep(1200);
        assertTrue("New core remains open", ValaniumService.core().isOpen());
        getInstrumentation().runOnMainSync(() ->
                assertEquals("Logout reaches entry screen", android.view.View.VISIBLE,
                        next.findViewById(R.id.screen_entry).getVisibility()));
        getInstrumentation().runOnMainSync(next::finish);
    }
}
