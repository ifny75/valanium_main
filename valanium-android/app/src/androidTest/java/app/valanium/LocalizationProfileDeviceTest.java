package app.valanium;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.test.InstrumentationTestCase;
import android.view.View;
import android.view.ViewGroup;
import android.widget.TextView;
import java.io.File;
import java.io.FileOutputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import org.json.JSONObject;

/** Exercises only the disposable QA package; no real account or contacts. */
public final class LocalizationProfileDeviceTest extends InstrumentationTestCase {
    private Object field(Object target, String name) throws Exception {
        Field f = MainActivity.class.getDeclaredField(name); f.setAccessible(true); return f.get(target);
    }
    private void set(Object target, String name, Object value) throws Exception {
        Field f = MainActivity.class.getDeclaredField(name); f.setAccessible(true); f.set(target, value);
    }
    private Object call(Activity activity, String name, Class<?>[] types, Object... args) throws Exception {
        Method m = MainActivity.class.getDeclaredMethod(name, types); m.setAccessible(true); return m.invoke(activity, args);
    }
    private boolean hasText(View view, String needle) {
        if (view instanceof TextView && ((TextView)view).getText().toString().contains(needle)) return true;
        if (view instanceof ViewGroup) for (int i=0;i<((ViewGroup)view).getChildCount();i++)
            if (hasText(((ViewGroup)view).getChildAt(i), needle)) return true;
        return false;
    }
    public void testEnglishProfileVerificationAndQrRoundTrip() throws Exception {
        Context context = getInstrumentation().getTargetContext();
        assertEquals("app.valanium.qa", context.getPackageName());
        context.getSharedPreferences("appearance",0).edit().putString("language","en").putBoolean("screen_privacy",false).commit();
        getInstrumentation().runOnMainSync(() -> I18n.apply(context));
        Activity activity = getInstrumentation().startActivitySync(new Intent(context,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK));
        getInstrumentation().waitForIdleSync();
        try {
            assertEquals("Settings", activity.getString(R.string.settings));
            assertEquals("Copy", I18n.t("Копировать"));
            String payload="valanium-verify:v1:"+"a".repeat(64);
            com.google.zxing.common.BitMatrix matrix=VerificationQr.encode(payload);
            int size=matrix.getWidth(); int[] pixels=new int[size*size];
            for(int y=0;y<size;y++) for(int x=0;x<size;x++) pixels[y*size+x]=matrix.get(x,y)?0xff000000:0xffffffff;
            assertEquals(payload,VerificationQr.decode(pixels,size,size));
            String peer="2".repeat(64);
            getInstrumentation().runOnMainSync(() -> {
                try {
                    set(activity,"currentPeer",peer);
                    call(activity,"showPeerCard",new Class<?>[]{String.class},peer);
                } catch(Exception e){throw new AssertionError(e);}
            });
            getInstrumentation().waitForIdleSync();
            Thread.sleep(350);
            Bitmap profileShot=getInstrumentation().getUiAutomation().takeScreenshot();
            try(FileOutputStream out=new FileOutputStream(new File(context.getExternalFilesDir(null),"english-profile-qa.png"))){profileShot.compress(Bitmap.CompressFormat.PNG,100,out);}
            profileShot.recycle();
            getInstrumentation().sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_BACK);
            getInstrumentation().runOnMainSync(() -> {
                try {
                    call(activity,"showVerification",new Class<?>[]{JSONObject.class},new JSONObject()
                        .put("peer_device",peer).put("safety_number","001002003 004005006 007008009")
                        .put("qr_payload",payload).put("verified",false).put("epoch",1).put("epoch_code","001002"));
                    AlertDialog dialog=(AlertDialog)field(activity,"verificationDialog");
                    assertTrue(dialog.isShowing());
                    assertTrue(hasText(dialog.getWindow().getDecorView(),"not yet been verified"));
                    assertTrue(hasText(dialog.getWindow().getDecorView(),"001002003"));
                } catch(Exception e){throw new AssertionError(e);}
            });
            getInstrumentation().waitForIdleSync();
            Thread.sleep(500); // Let the dialog entrance finish before visual evidence.
            Bitmap image=getInstrumentation().getUiAutomation().takeScreenshot();
            try(FileOutputStream output=new FileOutputStream(new File(context.getExternalFilesDir(null),"english-qr-qa.png"))) {
                image.compress(Bitmap.CompressFormat.PNG,100,output);
            }
            int[] rendered=new int[image.getWidth()*image.getHeight()];
            image.getPixels(rendered,0,image.getWidth(),0,0,image.getWidth(),image.getHeight());
            assertEquals("The QR must remain readable after rendering in the dialog",payload,VerificationQr.decode(rendered,image.getWidth(),image.getHeight()));
            image.recycle();
        } finally {
            getInstrumentation().runOnMainSync(() -> activity.finish());
            context.getSharedPreferences("appearance",0).edit().putString("language","ru").commit();
            getInstrumentation().runOnMainSync(() -> I18n.apply(context));
        }
    }
    public void testQrRejectsUnrelatedPayload() throws Exception {
        Context context=getInstrumentation().getTargetContext(); assertEquals("app.valanium.qa",context.getPackageName());
        com.google.zxing.common.BitMatrix matrix=VerificationQr.encode("https://example.org/not-a-verification");
        int size=matrix.getWidth();int[] pixels=new int[size*size];
        for(int y=0;y<size;y++)for(int x=0;x<size;x++)pixels[y*size+x]=matrix.get(x,y)?0xff000000:0xffffffff;
        try {VerificationQr.decode(pixels,size,size);fail("Unrelated QR must be rejected");}
        catch(IllegalArgumentException expected) {}
    }
}
