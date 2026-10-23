package app.valanium;

import com.google.zxing.BarcodeFormat;
import com.google.zxing.BinaryBitmap;
import com.google.zxing.DecodeHintType;
import com.google.zxing.EncodeHintType;
import com.google.zxing.RGBLuminanceSource;
import com.google.zxing.common.BitMatrix;
import com.google.zxing.common.HybridBinarizer;
import com.google.zxing.qrcode.QRCodeReader;
import com.google.zxing.qrcode.QRCodeWriter;
import java.util.Map;

final class VerificationQr {
    static BitMatrix encode(String payload) throws Exception {
        return new QRCodeWriter().encode(payload, BarcodeFormat.QR_CODE, 768, 768,
                Map.of(EncodeHintType.MARGIN, 4));
    }
    static String decode(int[] pixels, int width, int height) throws Exception {
        String value = new QRCodeReader().decode(new BinaryBitmap(new HybridBinarizer(
                new RGBLuminanceSource(width, height, pixels))), Map.of(DecodeHintType.TRY_HARDER, true)).getText();
        if (!value.matches("valanium-verify:v1:[0-9a-f]{64}")) throw new IllegalArgumentException("Not a verification QR");
        return value;
    }
}
