package app.valanium;
import org.junit.Test;
import static org.junit.Assert.*;
public class VerificationQrTest {
    @Test public void offlineQrRoundTripAndForeignPayloadRejection() throws Exception {
        String proof = "valanium-verify:v1:" + "a".repeat(64);
        com.google.zxing.common.BitMatrix bits = VerificationQr.encode(proof);
        int size = bits.getWidth();
        int[] pixels = new int[size * size];
        for (int y=0;y<size;y++) for(int x=0;x<size;x++) pixels[y*size+x]=bits.get(x,y)?0xff000000:0xffffffff;
        assertEquals(proof, VerificationQr.decode(pixels, size, size));
        bits = VerificationQr.encode("https://example.com/untrusted");
        for (int y=0;y<size;y++) for(int x=0;x<size;x++) pixels[y*size+x]=bits.get(x,y)?0xff000000:0xffffffff;
        try { VerificationQr.decode(pixels,size,size); fail("Foreign QR accepted"); }
        catch (IllegalArgumentException expected) { }
    }
}
