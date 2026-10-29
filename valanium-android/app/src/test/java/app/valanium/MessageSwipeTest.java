package app.valanium;

import org.junit.Test;
import static org.junit.Assert.*;

public class MessageSwipeTest {
    @Test public void verticalScrollLocksOutLaterHorizontalNoise() {
        MessageSwipe swipe = new MessageSwipe(6, 240);
        assertFalse(swipe.move(-4, 30));
        assertFalse(swipe.move(-180, 31));
        assertEquals(-1, swipe.release(false));
    }
    @Test public void rightSwipeAndTapNeverSelectAnything() {
        MessageSwipe swipe = new MessageSwipe(6, 240);
        assertFalse(swipe.move(100, 0));
        assertEquals(-1, swipe.release(false));
        swipe.reset();
        assertFalse(swipe.move(-6, 2));
        assertEquals(-1, swipe.release(false));
    }
    @Test public void selectionChangesOnlyAtThresholdsAndReleaseReturnsTheAction() {
        MessageSwipe swipe = new MessageSwipe(6, 240);
        assertTrue(swipe.move(-20, 1));
        assertEquals(-1, swipe.active);
        for (int i = 0; i < 6; i++) {
            swipe.move(-(44 + i * 39.2f), 3);
            assertEquals(i, swipe.active);
        }
        assertEquals(5, swipe.release(false));
        assertEquals(MessageSwipe.State.RELEASED, swipe.state);
        swipe.reset();
        assertEquals(0, swipe.offset, .001f);
        assertEquals(MessageSwipe.State.IDLE, swipe.state);
    }
    @Test public void cancellationNeverRunsAnActionAndNewGestureWorks() {
        MessageSwipe swipe = new MessageSwipe(3, 160);
        swipe.move(-140, 0);
        assertEquals(-1, swipe.release(true));
        swipe.reset(); swipe.move(-44, 1);
        assertEquals(0, swipe.release(false));
    }
    @Test public void unavailableActionsRedistributeThresholdsWithoutDeadSlots() {
        assertEquals(0, MessageSwipe.index(44, 3, 240));
        assertEquals(1, MessageSwipe.index(142, 3, 240));
        assertEquals(2, MessageSwipe.index(240, 3, 240));
        assertEquals(-1, MessageSwipe.index(240, 0, 240));
        assertEquals(0, MessageSwipe.index(240, 1, 240));
        assertEquals(5, MessageSwipe.index(44, 6, 44));
    }
    @Test public void distanceIsClampedAndRetreatCancelsSelection() {
        MessageSwipe swipe = new MessageSwipe(6, 240);
        swipe.move(-900, 0);
        assertEquals(-240, swipe.offset, .001f);
        assertEquals(5, swipe.active);
        swipe.move(-30, 0);
        assertEquals(-1, swipe.release(false));
    }
    @Test public void incomingShortBubbleCanSelectEveryActionBySwipingRight() {
        MessageSwipe swipe = new MessageSwipe(6, 240, 1);
        assertFalse(swipe.move(-80, 0));
        for (int i = 0; i < 6; i++) {
            assertTrue(swipe.move(44 + i * 39.2f, 1));
            assertEquals(i, swipe.active);
        }
        assertEquals(5, swipe.release(false));
    }
    @Test public void longBubblesStayWithinRowWhileEveryActionRemainsReachable() {
        for (int direction : new int[]{-1, 1}) {
            MessageSwipe swipe = new MessageSwipe(6, 240, direction);
            swipe.move(direction * 240, 0);
            assertEquals(5, swipe.active);
            float left = direction < 0 ? 48 : 0;
            float right = direction < 0 ? 360 : 312;
            float travel = MessageSwipe.boundedOffset(swipe.offset, left, right, 360);
            assertTrue(left + travel >= 0);
            assertTrue(right + travel <= 360);
            assertEquals(direction * 48, travel, .001f);
        }
    }
    @Test public void bubbleBoundsHoldForShortAndFullWidthMessages() {
        for (int width : new int[]{48, 320, 600}) {
            for (int length : new int[]{20, 40, width}) {
                if (length > width) continue;
                for (int left = 0; left + length <= width; left += 4) {
                    for (int requested : new int[]{-900, -20, 0, 20, 900}) {
                        float travel = MessageSwipe.boundedOffset(requested, left, left + length, width);
                        assertTrue(left + travel >= 0);
                        assertTrue(left + length + travel <= width);
                    }
                }
            }
        }
    }
}
