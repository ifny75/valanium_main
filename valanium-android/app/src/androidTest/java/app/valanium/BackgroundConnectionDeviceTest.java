package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.test.InstrumentationTestCase;
import java.lang.reflect.Field;

/** Lifecycle coverage in the isolated package, with no user accounts or radio changes. */
public final class BackgroundConnectionDeviceTest extends InstrumentationTestCase {
    private Object field(String name)throws Exception {
        Field field=ValaniumService.class.getDeclaredField(name);field.setAccessible(true);return field.get(null);
    }
    public void testNetworkWatcherSurvivesBackgroundAndResume() throws Exception {
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        context.getSharedPreferences("appearance",0).edit().putString("background_delivery","persistent").commit();
        Activity activity=getInstrumentation().startActivitySync(new Intent(context,MainActivity.class)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK|Intent.FLAG_ACTIVITY_CLEAR_TASK));
        try {
            for(int i=0;i<50&&field("networkCallback")==null;i++)Thread.sleep(100);
            Object watcher=field("networkCallback");assertNotNull(watcher);
            assertTrue("Persistent delivery must work even without drawer notification permission",ValaniumService.active());
            getInstrumentation().runOnMainSync(()->activity.moveTaskToBack(true));
            getInstrumentation().waitForIdleSync();Thread.sleep(400);
            assertSame("Activity.onStop must not unregister network recovery",watcher,field("networkCallback"));
            context.startActivity(new Intent(context,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK|Intent.FLAG_ACTIVITY_SINGLE_TOP));
            getInstrumentation().waitForIdleSync();Thread.sleep(400);
            assertSame("Resume must not duplicate the network watcher",watcher,field("networkCallback"));
            assertTrue(ValaniumService.core().isOpen());
        } finally {getInstrumentation().runOnMainSync(activity::finish);}
    }
}
