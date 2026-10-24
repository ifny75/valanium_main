package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.test.InstrumentationTestCase;
import android.widget.TextView;

import java.lang.reflect.Method;
import java.util.regex.Pattern;

/** Не позволяет снова вывести публичные IP инфраструктуры на экран подключения. */
public final class NodeAddressVisibilityDeviceTest extends InstrumentationTestCase {
    private static final Pattern IPV4 =
            Pattern.compile("\\b(?:[0-9]{1,3}\\.){3}[0-9]{1,3}\\b");

    private void call(Activity activity, String name) {
        try {
            Method method = MainActivity.class.getDeclaredMethod(name);
            method.setAccessible(true);
            method.invoke(activity);
        } catch (Exception error) {
            throw new AssertionError(error);
        }
    }

    public void testConnectionOverviewContainsNoInfrastructureIp() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("Refusing real account", "app.valanium.qa", context.getPackageName());
        context.getSharedPreferences("appearance", 0).edit()
                .putString("transport", "multihop")
                .putString("multihop_node", "alpha")
                .commit();

        Activity activity = getInstrumentation().startActivitySync(new Intent(context,
                MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        getInstrumentation().waitForIdleSync();
        getInstrumentation().runOnMainSync(() -> call(activity, "renderConnectionOverview"));

        String visible = ((TextView) activity.findViewById(R.id.connection_route_summary)).getText()
                + "\n" + ((TextView) activity.findViewById(R.id.connection_destination)).getText()
                + "\n" + ((TextView) activity.findViewById(R.id.connection_nodes)).getText();
        assertFalse("Infrastructure IP must never be rendered: " + visible,
                IPV4.matcher(visible).find());
        assertTrue(visible.contains(context.getString(R.string.connection_addresses_hidden)));
        getInstrumentation().runOnMainSync(activity::finish);
    }
}
