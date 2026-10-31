package app.valanium;

import org.junit.Test;
import static org.junit.Assert.*;

public class AvatarTransformTest {
    @Test public void smallImagesFillTheWholeAvatar() {
        assertArrayEquals(new float[]{32, 0, 0}, AvatarTransform.fit(2, 2, 64, 64), .001f);
    }
    @Test public void landscapeAndPortraitStayCentered() {
        assertArrayEquals(new float[]{.2f, -32, 0}, AvatarTransform.fit(640, 320, 64, 64), .001f);
        assertArrayEquals(new float[]{.2f, 0, -32}, AvatarTransform.fit(320, 640, 64, 64), .001f);
    }
    @Test public void differentViewSizesRecalculateTheScale() {
        assertArrayEquals(new float[]{2, 0, 0}, AvatarTransform.fit(64, 64, 128, 128), .001f);
    }
}
