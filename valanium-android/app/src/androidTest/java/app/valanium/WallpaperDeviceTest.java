package app.valanium;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.*;
import android.graphics.drawable.Drawable;
import android.test.InstrumentationTestCase;
import android.view.View;
import java.io.File;
import java.io.FileOutputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;

public final class WallpaperDeviceTest extends InstrumentationTestCase {
    private Object call(Activity activity,String name,Class<?>[] types,Object... args)throws Exception{
        Method method=MainActivity.class.getDeclaredMethod(name,types);method.setAccessible(true);return method.invoke(activity,args);
    }
    private Activity start(){
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        context.getSharedPreferences("appearance",0).edit().putInt("wallpaper_intensity",100).putBoolean("screen_privacy",false).commit();
        return getInstrumentation().startActivitySync(new Intent(context,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK|Intent.FLAG_ACTIVITY_CLEAR_TASK));
    }
    public void testCustomPhotoPersistsFillsTheWindowAndCanBeRemoved() throws Exception {
        Activity first=start();getInstrumentation().waitForIdleSync();
        Bitmap image=Bitmap.createBitmap(128,64,Bitmap.Config.ARGB_8888);image.eraseColor(0xff397ba8);
        call(first,"saveCustomWallpaper",new Class<?>[]{Bitmap.class},image);getInstrumentation().waitForIdleSync();
        File file=(File)call(first,"customWallpaperFile",new Class<?>[]{});assertTrue(file.length()>0);
        getInstrumentation().runOnMainSync(first::finish);
        Activity activity=start();getInstrumentation().waitForIdleSync();Thread.sleep(500);
        getInstrumentation().runOnMainSync(()->{
            try {
                call(activity,"show",new Class<?>[]{View.class},activity.findViewById(R.id.screen_conversation));
                call(activity,"layoutChatWallpaper",new Class<?>[]{});
                Field field=MainActivity.class.getDeclaredField("chatWallpaperLayer");field.setAccessible(true);
                View layer=(View)field.get(activity),root=activity.findViewById(R.id.app_root);
                assertEquals(0f,layer.getX(),1f);assertEquals(0f,layer.getY(),1f);
                assertEquals(root.getWidth(),layer.getLayoutParams().width);
                assertEquals(root.getHeight(),layer.getLayoutParams().height);
                Drawable drawable=layer.getBackground();assertNotNull(drawable);
                Bitmap output=Bitmap.createBitmap(200,400,Bitmap.Config.ARGB_8888);
                drawable.setBounds(0,0,200,400);drawable.draw(new Canvas(output));
                for(int[] point:new int[][]{{0,0},{199,0},{0,399},{199,399}}){
                    int pixel=output.getPixel(point[0],point[1]);assertEquals(255,Color.alpha(pixel));
                    assertEquals(57,Color.red(pixel),3);assertEquals(123,Color.green(pixel),3);assertEquals(168,Color.blue(pixel),3);
                }
                drawable.setBounds(0,0,root.getWidth(),root.getHeight());output.recycle();
            }catch(Exception error){throw new AssertionError(error);}
        });
        Thread.sleep(300);Bitmap shot=getInstrumentation().getUiAutomation().takeScreenshot();
        try(FileOutputStream output=new FileOutputStream(new File(activity.getExternalFilesDir(null),"wallpaper-qa.png"))){shot.compress(Bitmap.CompressFormat.PNG,100,output);}shot.recycle();
        getInstrumentation().runOnMainSync(()->{
            try{Field field=MainActivity.class.getDeclaredField("customWallpaperRemove");field.setAccessible(true);((View)field.get(activity)).performClick();}
            catch(Exception error){throw new AssertionError(error);}
        });
        assertFalse(file.exists());getInstrumentation().runOnMainSync(activity::finish);
    }
    public void testGradientPresetsReachAllFourCorners() throws Exception {
        Activity activity=start();
        for(String preset:new String[]{"aurora","mesh"}){
            Drawable drawable=(Drawable)call(activity,"wallpaperPattern",new Class<?>[]{String.class,int.class},preset,400);
            Bitmap output=Bitmap.createBitmap(400,900,Bitmap.Config.ARGB_8888);
            drawable.setBounds(0,0,400,900);drawable.draw(new Canvas(output));
            for(int[] point:new int[][]{{0,0},{399,0},{0,899},{399,899}})assertTrue(preset+" has an empty corner",Color.alpha(output.getPixel(point[0],point[1]))>0);
            output.recycle();
        }
        getInstrumentation().runOnMainSync(activity::finish);
    }
}
