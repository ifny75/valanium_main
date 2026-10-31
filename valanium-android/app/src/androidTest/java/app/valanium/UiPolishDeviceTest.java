package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.test.InstrumentationTestCase;
import android.view.MotionEvent;
import android.view.View;
import android.widget.TextView;
import java.io.File;
import java.io.FileOutputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.Map;
import org.json.JSONObject;

public final class UiPolishDeviceTest extends InstrumentationTestCase {
    private Object field(Object object,String name)throws Exception{Field f=object.getClass().getDeclaredField(name);f.setAccessible(true);return f.get(object);}
    private Object call(Object object,String name,Class<?>[] types,Object... values)throws Exception{Method m=object.getClass().getDeclaredMethod(name,types);m.setAccessible(true);return m.invoke(object,values);}
    private Activity start() {
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        context.getSharedPreferences("appearance",0).edit().putString("language","ru").putBoolean("screen_privacy",false).commit();
        getInstrumentation().runOnMainSync(()->I18n.apply(context));
        return getInstrumentation().startActivitySync(new Intent(context,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK|Intent.FLAG_ACTIVITY_CLEAR_TASK));
    }
    @SuppressWarnings("unchecked")
    public void testThemeHeaderAndExpiringTyping() throws Exception {
        Activity activity=start();getInstrumentation().waitForIdleSync();
        getInstrumentation().runOnMainSync(()->{
            try{
                int green=Color.rgb(103,212,163);
                call(activity,"setAccent",new Class<?>[]{int.class},green);
                assertEquals(green,activity.findViewById(R.id.entry_submit).getBackgroundTintList().getDefaultColor());
                assertTrue(activity.findViewById(R.id.chat_logo) instanceof TextView);
                assertEquals(View.GONE,activity.findViewById(R.id.status_text).getVisibility());
                call(activity,"setTheme",new Class<?>[]{String.class},"light");
                assertEquals(Color.rgb(16,16,16),((TextView)activity.findViewById(R.id.profile_name)).getCurrentTextColor());
                call(activity,"setTheme",new Class<?>[]{String.class},"dark");
                Field peer=MainActivity.class.getDeclaredField("currentPeer");peer.setAccessible(true);peer.set(activity,"2".repeat(64));
                call(activity,"show",new Class<?>[]{View.class},activity.findViewById(R.id.screen_conversation));
                ((MainActivity)activity).onEvent(new JSONObject().put("type","peer_typing").put("peer_device","2".repeat(64)).put("active",true));
                assertEquals(View.VISIBLE,activity.findViewById(R.id.peer_typing_dots).getVisibility());
                ((Map<String,Long>)field(activity,"peerTypingUntil")).put("2".repeat(64),0L);
                call(activity,"refreshPeerActivity",new Class<?>[]{});
                assertEquals(View.GONE,activity.findViewById(R.id.peer_typing_dots).getVisibility());
                ((MainActivity)activity).onEvent(new JSONObject().put("type","peer_online").put("peer_device","2".repeat(64)));
                assertTrue(((TextView)activity.findViewById(R.id.peer_state)).getText().toString().contains("В сети недавно"));
            }catch(Exception e){throw new AssertionError(e);}
        });
        getInstrumentation().runOnMainSync(activity::finish);
    }
    public void testPhotoZoomPanAndExportStayInsideSource() throws Exception {
        Activity activity=start();getInstrumentation().waitForIdleSync();
        Bitmap source=Bitmap.createBitmap(800,600,Bitmap.Config.ARGB_8888);
        Canvas canvas=new Canvas(source);Paint p=new Paint();p.setColor(0xff397ba8);canvas.drawRect(0,0,800,600,p);
        p.setColor(0xffe6ac58);canvas.drawCircle(400,300,180,p);p.setColor(0xff6ccca2);canvas.drawRect(100,100,230,470,p);
        PhotoEditor editor=new PhotoEditor(activity,source,1f,1_500_000,(body,width,height)->{});
        getInstrumentation().runOnMainSync(editor::show);getInstrumentation().waitForIdleSync();Thread.sleep(400);
        getInstrumentation().runOnMainSync(()->{
            try{
                View view=(View)field(editor,"view");assertTrue(view.getWidth()>0);
                RectF original=new RectF((RectF)field(editor,"crop"));
                call(view,"zoomTo",new Class<?>[]{float.class,float.class,float.class},3f,view.getWidth()/2f,view.getHeight()/2f);
                RectF crop=(RectF)field(editor,"crop");
                assertEquals(original.width()/3,crop.width(),2f);assertEquals(crop.width(),crop.height(),1f);
                RectF screen=new RectF((RectF)call(view,"cropOnScreen",new Class<?>[]{}));
                long now=android.os.SystemClock.uptimeMillis();
                MotionEvent down=MotionEvent.obtain(now,now,MotionEvent.ACTION_DOWN,screen.centerX(),screen.centerY(),0);
                view.dispatchTouchEvent(down);down.recycle();
                MotionEvent move=MotionEvent.obtain(now,now+30,MotionEvent.ACTION_MOVE,screen.centerX()+30,screen.centerY(),0);
                view.dispatchTouchEvent(move);move.recycle();
                MotionEvent up=MotionEvent.obtain(now,now+50,MotionEvent.ACTION_UP,screen.centerX()+30,screen.centerY(),0);
                view.dispatchTouchEvent(up);up.recycle();
                RectF after=(RectF)call(view,"cropOnScreen",new Class<?>[]{});
                assertEquals("Dragging a zoomed photo keeps the crop frame fixed",screen.left,after.left,1f);
                for(float[] step:new float[][]{{4,0,0},{2,view.getWidth(),view.getHeight()},
                        {5,0,view.getHeight()},{1,view.getWidth(),0}}){
                    RectF beforeZoom=new RectF((RectF)call(view,"cropOnScreen",new Class<?>[]{}));
                    call(view,"zoomTo",new Class<?>[]{float.class,float.class,float.class},step[0],step[1],step[2]);
                    RectF afterZoom=(RectF)call(view,"cropOnScreen",new Class<?>[]{});
                    assertEquals("Pinching at an edge keeps the frame fixed",beforeZoom.left,afterZoom.left,1f);
                    assertEquals(beforeZoom.top,afterZoom.top,1f);
                    assertEquals(beforeZoom.right,afterZoom.right,1f);
                    assertEquals(beforeZoom.bottom,afterZoom.bottom,1f);
                }
                crop=(RectF)field(editor,"crop");assertTrue(crop.left>=0&&crop.top>=0&&crop.right<=800&&crop.bottom<=600);
                Object rendered=call(editor,"render",new Class<?>[]{});assertNotNull(rendered);
                assertEquals(field(rendered,"width"),field(rendered,"height"));
                Field handle=view.getClass().getDeclaredField("handle");handle.setAccessible(true);
                Field initial=view.getClass().getDeclaredField("startCrop");initial.setAccessible(true);
                for(String corner:new String[]{"nw","ne","sw","se"}) {
                    handle.set(view,corner);initial.set(view,new RectF(crop));
                    RectF resized=(RectF)call(view,"resized",new Class<?>[]{float.class,float.class},2000f,-2000f);
                    assertTrue(resized.left>=0&&resized.top>=0&&resized.right<=800&&resized.bottom<=600);
                    assertEquals(resized.width(),resized.height(),1f);
                }
            }catch(Exception e){throw new AssertionError(e);}
        });
        Thread.sleep(300);Bitmap screenshot=getInstrumentation().getUiAutomation().takeScreenshot();
        try(FileOutputStream output=new FileOutputStream(new File(activity.getExternalFilesDir(null),"photo-polish-qa.png"))){screenshot.compress(Bitmap.CompressFormat.PNG,100,output);}screenshot.recycle();
        getInstrumentation().runOnMainSync(activity::finish);
    }
    public void testSmallAvatarScalesAcrossItsWholeBounds() {
        Bitmap source=Bitmap.createBitmap(2,2,Bitmap.Config.ARGB_8888);
        source.setPixel(0,0,Color.RED);source.setPixel(1,0,Color.GREEN);
        source.setPixel(0,1,Color.BLUE);source.setPixel(1,1,Color.YELLOW);
        AvatarDrawable avatar=new AvatarDrawable(source,0);
        Bitmap output=Bitmap.createBitmap(64,64,Bitmap.Config.ARGB_8888);
        Canvas canvas=new Canvas(output);avatar.setBounds(0,0,64,64);avatar.draw(canvas);
        assertEquals(Color.RED,output.getPixel(8,8));assertEquals(Color.GREEN,output.getPixel(56,8));
        assertEquals(Color.BLUE,output.getPixel(8,56));assertEquals(Color.YELLOW,output.getPixel(56,56));
        canvas.drawColor(Color.TRANSPARENT,android.graphics.PorterDuff.Mode.CLEAR);
        avatar.setBounds(4,6,36,38);avatar.draw(canvas);
        assertEquals(Color.TRANSPARENT,output.getPixel(0,0));
        assertEquals(Color.RED,output.getPixel(8,10));assertEquals(Color.YELLOW,output.getPixel(32,34));
        output.recycle();source.recycle();
    }
    public void testAvatarExportUsesACompactThumbnail() throws Exception {
        Activity activity=start();Bitmap source=Bitmap.createBitmap(1600,1000,Bitmap.Config.ARGB_8888);source.eraseColor(0xff397ba8);
        try{
            PhotoEditor editor=new PhotoEditor(activity,source,1f,80_000,384,(body,width,height)->{});
            call(editor,"rebuildWork",new Class<?>[]{});call(editor,"resetCrop",new Class<?>[]{});
            Object rendered=call(editor,"render",new Class<?>[]{});assertNotNull(rendered);
            assertEquals(384,field(rendered,"width"));assertEquals(384,field(rendered,"height"));
        }finally{getInstrumentation().runOnMainSync(activity::finish);}
    }
    public void testLargePhotoCanRotateAgainWithoutRecyclingItsSource() throws Exception {
        Activity activity=start();
        Bitmap source=Bitmap.createBitmap(2048,1536,Bitmap.Config.ARGB_8888);
        PhotoEditor editor=new PhotoEditor(activity,source,1f,1_500_000,(body,width,height)->{});
        call(editor,"rebuildWork",new Class<?>[]{});
        assertFalse("Downsampling must preserve the source needed for later rotations",source.isRecycled());
        Field rotation=PhotoEditor.class.getDeclaredField("rotation");rotation.setAccessible(true);rotation.set(editor,90);
        call(editor,"rebuildWork",new Class<?>[]{});
        assertFalse(source.isRecycled());
        Bitmap work=(Bitmap)field(editor,"work");assertTrue(Math.max(work.getWidth(),work.getHeight())<=1600);
        if(work!=source)work.recycle();source.recycle();
        getInstrumentation().runOnMainSync(activity::finish);
    }
    public void testCallPreviewDoesNotRequestTheMicrophone() throws Exception {
        Activity activity=start();getInstrumentation().waitForIdleSync();
        int before=activity.checkSelfPermission(android.Manifest.permission.RECORD_AUDIO);
        getInstrumentation().runOnMainSync(()->{
            try {
                Field peer=MainActivity.class.getDeclaredField("currentPeer");peer.setAccessible(true);peer.set(activity,"2".repeat(64));
                call(activity,"showCallPreview",new Class<?>[]{});
                assertEquals(before,activity.checkSelfPermission(android.Manifest.permission.RECORD_AUDIO));
            }catch(Exception e){throw new AssertionError(e);}
        });
        Thread.sleep(400);Bitmap screenshot=getInstrumentation().getUiAutomation().takeScreenshot();
        try(FileOutputStream output=new FileOutputStream(new File(activity.getExternalFilesDir(null),"calls-preview-qa.png"))){screenshot.compress(Bitmap.CompressFormat.PNG,100,output);}screenshot.recycle();
        getInstrumentation().runOnMainSync(activity::finish);
    }
}
