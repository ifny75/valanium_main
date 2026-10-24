package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.test.InstrumentationTestCase;
import android.view.View;
import android.widget.ListView;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.List;
import java.util.Map;

/** Проверяет, что длинные ленты не превращаются в сотни одновременно прикреплённых View. */
public final class VirtualListsDeviceTest extends InstrumentationTestCase {
    private Object call(Activity activity, String name, Class<?>[] types, Object... args) {
        try {
            Method method = MainActivity.class.getDeclaredMethod(name, types);
            method.setAccessible(true);
            return method.invoke(activity, args);
        } catch (Exception error) {
            throw new AssertionError(error);
        }
    }

    private Object field(Object target, String name) {
        try {
            Field field = target.getClass().getDeclaredField(name);
            field.setAccessible(true);
            return field.get(target);
        } catch (Exception error) {
            throw new AssertionError(error);
        }
    }

    private void setField(Object target, String name, Object value) {
        try {
            Field field = target.getClass().getDeclaredField(name);
            field.setAccessible(true);
            field.set(target, value);
        } catch (Exception error) {
            throw new AssertionError(error);
        }
    }

    @SuppressWarnings("unchecked")
    public void testLongChatAndConversationAttachOnlyVisibleRows() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("Refusing real account", "app.valanium.qa", context.getPackageName());
        Activity activity = getInstrumentation().startActivitySync(new Intent(context,
                MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        Thread.sleep(1200);

        getInstrumentation().runOnMainSync(() -> {
            Map<String, String> conversations =
                    (Map<String, String>) field(activity, "conversations");
            conversations.clear();
            for (int i = 0; i < 240; i++) {
                conversations.put(String.format("%064x", i + 1), "conversation-" + i);
            }
            call(activity, "renderPeers", new Class<?>[]{});
            call(activity, "show", new Class<?>[]{View.class},
                    activity.findViewById(R.id.screen_chat));
        });
        getInstrumentation().waitForIdleSync();

        ListView chats = activity.findViewById(R.id.contact_list);
        assertEquals(240, chats.getAdapter().getCount());
        assertTrue("Only visible chat rows are attached",
                chats.getChildCount() > 0 && chats.getChildCount() < 40);

        getInstrumentation().runOnMainSync(() -> {
            try {
                String peer = String.format("%064x", 1);
                setField(activity, "currentPeer", peer);
                Object page = call(activity, "page", new Class<?>[]{String.class},
                        "conversation-0");
                List<Object> timeline = (List<Object>) field(page, "timeline");
                Class<?> itemClass = Class.forName("app.valanium.MainActivity$TimelineItem");
                Method message = itemClass.getDeclaredMethod("message", String.class,
                        boolean.class, long.class, String.class);
                message.setAccessible(true);
                long base = System.currentTimeMillis() - 300_000L;
                for (int i = 0; i < 320; i++) {
                    timeline.add(message.invoke(null, "Сообщение " + i, i % 2 == 0,
                            base + i * 1000L, "message-" + i));
                }
                call(activity, "paintConversation", new Class<?>[]{String.class},
                        "conversation-0");
                call(activity, "show", new Class<?>[]{View.class},
                        activity.findViewById(R.id.screen_conversation));
            } catch (Exception error) {
                throw new AssertionError(error);
            }
        });
        getInstrumentation().waitForIdleSync();

        ListView messages = activity.findViewById(R.id.messages_scroll);
        assertEquals(320, messages.getAdapter().getCount());
        assertTrue("Only visible message rows are attached",
                messages.getChildCount() > 0 && messages.getChildCount() < 40);
        getInstrumentation().runOnMainSync(activity::finish);
    }
}
