package app.valanium;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Context;
import android.content.Intent;
import android.test.InstrumentationTestCase;
import android.view.View;
import android.view.ViewGroup;
import android.widget.TextView;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.Map;
import org.json.JSONObject;

/** UI checks in the isolated app; no real account or outbound messages. */
public final class DeliveryAndVerificationDeviceTest extends InstrumentationTestCase {
    private Object field(Object target, String name) {
        try { Field f=target.getClass().getDeclaredField(name); f.setAccessible(true); return f.get(target); }
        catch(Exception e){throw new AssertionError(e);}
    }
    private void set(Object target,String name,Object value) {
        try {Field f=target.getClass().getDeclaredField(name);f.setAccessible(true);f.set(target,value);}
        catch(Exception e){throw new AssertionError(e);}
    }
    private Object call(Activity activity,String name,Class<?>[] types,Object... args) {
        try {Method m=MainActivity.class.getDeclaredMethod(name,types);m.setAccessible(true);return m.invoke(activity,args);}
        catch(Exception e){throw new AssertionError(e);}
    }
    private boolean hasText(View view,String needle) {
        if(view instanceof TextView && ((TextView)view).getText().toString().contains(needle))return true;
        if(view instanceof ViewGroup)for(int i=0;i<((ViewGroup)view).getChildCount();i++)if(hasText(((ViewGroup)view).getChildAt(i),needle))return true;
        return false;
    }
    @SuppressWarnings("unchecked")
    public void testEvidenceLabelsAndPairBoundDialog() throws Exception {
        Context context=getInstrumentation().getTargetContext();
        assertEquals("Refusing real account","app.valanium.qa",context.getPackageName());
        context.getSharedPreferences("appearance",0).edit().putBoolean("screen_privacy",false).commit();
        Activity activity=getInstrumentation().startActivitySync(new Intent(context,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK));
        Thread.sleep(1400);
        String peer=String.format("%064x",2);
        String other=String.format("%064x",3);
        String conversation="aabb";
        String body="\u2063OBS1:{\"type\":\"text\",\"id\":\"qa-message\",\"text\":\"Проверка очереди\"}";
        getInstrumentation().runOnMainSync(()->{
            set(activity,"currentPeer",peer);
            set(activity,"foregroundAuthorized",true);
            ((Map<String,String>)field(activity,"conversations")).put(peer,conversation);
            View saving=(View)call(activity,"buildBubble",new Class<?>[]{String.class,boolean.class,String.class},body,true,peer);
            assertTrue(hasText(saving,"Сохраняется"));
            try {
                ((MainActivity)activity).onEvent(new JSONObject().put("type","delivery").put("conversation",conversation).put("id","qa-message").put("state","queued"));
                View queued=(View)call(activity,"buildBubble",new Class<?>[]{String.class,boolean.class,String.class},body,true,peer);
                assertTrue(hasText(queued,"В очереди"));
                assertFalse(hasText(queued,"Принято сервером"));
                JSONObject verify=new JSONObject().put("peer_device",peer).put("safety_number","001002003 004005006 007008009")
                        .put("qr_payload","valanium-verify:v1:"+"a".repeat(64)).put("verified",false).put("epoch",1).put("epoch_code","001002");
                call(activity,"showVerification",new Class<?>[]{JSONObject.class},verify);
                AlertDialog dialog=(AlertDialog)field(activity,"verificationDialog");
                assertTrue(dialog.isShowing());
                assertTrue(hasText(dialog.getWindow().getDecorView(),"Устройство ещё не проверено"));
                call(activity,"showVerification",new Class<?>[]{JSONObject.class},new JSONObject(verify.toString()).put("peer_device",other));
                assertSame("Late reply must not replace the current peer",dialog,field(activity,"verificationDialog"));
            } catch(Exception e){throw new AssertionError(e);}
        });
        Thread.sleep(300);
        android.graphics.Bitmap screenshot=getInstrumentation().getUiAutomation().takeScreenshot();
        assertNotNull(screenshot);
        try(java.io.FileOutputStream out=new java.io.FileOutputStream(new java.io.File(context.getExternalFilesDir(null),"verification-078.png"))){screenshot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,out);}
        getInstrumentation().runOnMainSync(()->{
            ((AlertDialog)field(activity,"verificationDialog")).dismiss();
            activity.finish();
        });
    }
}
