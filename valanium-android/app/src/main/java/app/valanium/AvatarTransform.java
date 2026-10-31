package app.valanium;

/** Center-crop in destination pixels; independent of bitmap density. */
final class AvatarTransform {
    static float[] fit(int imageWidth, int imageHeight, float width, float height) {
        if (imageWidth <= 0 || imageHeight <= 0 || width <= 0 || height <= 0)
            throw new IllegalArgumentException("Invalid avatar bounds");
        float scale = Math.max(width / imageWidth, height / imageHeight);
        return new float[]{scale, (width - imageWidth * scale) / 2,
                (height - imageHeight * scale) / 2};
    }
}
