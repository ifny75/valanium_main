package app.valanium;

/** Gesture policy in dp. Vertical movement locks out selection until the next touch. */
final class MessageSwipe {
    enum State { IDLE, DRAGGING, ACTION_SELECTED, RELEASED, RESET }
    State state = State.IDLE;
    private boolean vertical;
    private final int count;
    private final float maximum;
    float offset;
    int active = -1;

    MessageSwipe(int count, float maximum) {
        this.count = count;
        this.maximum = Math.max(44f, maximum);
    }

    boolean move(float dx, float dy) {
        if (vertical || count == 0) return false;
        if (state == State.IDLE) {
            if (Math.abs(dy) > 10 && Math.abs(dy) > Math.abs(dx)) {
                vertical = true;
                return false;
            }
            if (dx >= -12 || Math.abs(dx) <= Math.abs(dy)) return false;
            state = State.DRAGGING;
        }
        offset = Math.max(-maximum, Math.min(0, dx));
        active = index(-offset, count, maximum);
        state = active < 0 ? State.DRAGGING : State.ACTION_SELECTED;
        return true;
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
