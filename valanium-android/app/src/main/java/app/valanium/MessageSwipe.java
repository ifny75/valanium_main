package app.valanium;

/** Gesture policy in dp. Vertical movement locks out selection until the next touch. */
final class MessageSwipe {
    enum State { IDLE, DRAGGING, ACTION_SELECTED, RELEASED, RESET }
    State state = State.IDLE;
    private boolean vertical;
    private final int count;
    private final float maximum;
    private final int direction;
    float offset;
    int active = -1;

    MessageSwipe(int count, float maximum) {
        this(count, maximum, -1);
    }

    MessageSwipe(int count, float maximum, int direction) {
        this.count = count;
        this.maximum = Math.max(44f, maximum);
        this.direction = direction < 0 ? -1 : 1;
    }

    boolean move(float dx, float dy) {
        if (vertical || count == 0) return false;
        if (state == State.IDLE) {
            if (Math.abs(dy) > 10 && Math.abs(dy) > Math.abs(dx)) {
                vertical = true;
                return false;
            }
            if (dx * direction <= 12 || Math.abs(dx) <= Math.abs(dy)) return false;
            state = State.DRAGGING;
        }
        float distance = Math.max(0, Math.min(maximum, dx * direction));
        offset = direction * distance;
        active = index(distance, count, maximum);
        state = active < 0 ? State.DRAGGING : State.ACTION_SELECTED;
        return true;
    }

    // Selection distance is independent of visual travel. Long bubbles may have
    // little room, but all actions must remain reachable without clipping text.
    static float boundedOffset(float offset, float left, float right, float width) {
        return Math.max(-Math.max(0, left), Math.min(Math.max(0, width - right), offset));
    }

    static int index(float distance, int count, float maximum) {
        float first = Math.min(44, maximum / 3);
        if (count == 0 || distance < first) return -1;
        if (count == 1) return 0;
        float step = (maximum - first) / (count - 1);
        return Math.min(count - 1, (int) ((distance - first) / step + .0001f));
    }

    int release(boolean cancelled) {
        int selected = cancelled ? -1 : active;
        state = State.RELEASED;
        active = -1;
        return selected;
    }

    void reset() {
        state = State.RESET;
        offset = 0;
        active = -1;
        vertical = false;
        state = State.IDLE;
    }
}
