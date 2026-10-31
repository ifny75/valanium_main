package app.valanium;

import android.content.Context;
import android.app.job.JobScheduler;
import android.os.ParcelFileDescriptor;
import android.test.InstrumentationTestCase;
import app.valanium.core.Core;
import java.io.File;
import java.io.FileInputStream;
import org.json.JSONObject;

public final class BackgroundDeliveryDeviceTest extends InstrumentationTestCase {
    public void testCancelledWorkDoesNotStartAConnection(){
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        assertFalse(BackgroundDelivery.connect(context,ValaniumService.core(),()->false));
    }
    public void testLockedColdAccountIsNotOpenedByScheduledWork() throws Exception {
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        ValaniumService.core().close();android.content.SharedPreferences prefs=context.getSharedPreferences("valanium.secure.local",0);
        int old=prefs.getInt("lock_seconds",0);
        try{prefs.edit().putInt("lock_seconds",60).commit();assertFalse(BackgroundDelivery.openExisting(context,ValaniumService.core()));assertFalse(ValaniumService.core().isOpen());}
        finally{prefs.edit().putInt("lock_seconds",old).commit();}
    }
    public void testPeriodicJobRestoresAnAccountAndReceivesAnOfflineMessage() throws Exception {
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        assertFalse(ValaniumService.active());assertFalse(Events.hasListeners());
        assertFalse(Core.startTor(new File(context.getFilesDir(),"tor").getAbsolutePath()).isEmpty());
        File database=new File(context.getFilesDir(),"background-live-"+System.nanoTime()+".db");
        File peerDatabase=new File(context.getCacheDir(),"background-peer-"+System.nanoTime()+".db");
        String oldDatabase=context.getSharedPreferences("account_session",0).getString("database","valanium.db");
        String oldMode=BackgroundDelivery.mode(context);
        LocalSecretStore secrets=new LocalSecretStore(context);String oldSecret=secrets.load();
        try(LiveTorDeliveryDeviceTest.Client alice=new LiveTorDeliveryDeviceTest.Client(database);
            LiveTorDeliveryDeviceTest.Client bob=new LiveTorDeliveryDeviceTest.Client(peerDatabase)){
            alice.register();bob.register();
            alice.send(new JSONObject().put("type","directory_set").put("device",bob.device).put("standing","approved"));
            bob.send(new JSONObject().put("type","directory_set").put("device",alice.device).put("standing","approved"));
            String first="background-initial-"+System.nanoTime();
            alice.send(new JSONObject().put("type","send").put("recipient_device",bob.device).put("body",first));
            bob.waitFor(e->"message".equals(e.optString("type"))&&first.equals(e.optString("body")));
            String thread=alice.waitFor(e->"conversation_started".equals(e.optString("type"))&&bob.device.equals(e.optString("peer_device"))).getString("conversation");
            alice.close();
            String offline="background-offline-"+System.nanoTime();
            String offlineBody="\u2063OBS1:"+new JSONObject().put("v",1).put("type","text").put("id",offline).put("text",offline);
            bob.send(new JSONObject().put("type","send").put("recipient_device",alice.device).put("body",offlineBody));
            bob.waitFor(e->"delivery".equals(e.optString("type"))&&offline.equals(e.optString("id"))&&"accepted".equals(e.optString("state")));
            context.getSharedPreferences("account_session",0).edit().putString("database",database.getName()).commit();
            context.getSharedPreferences("appearance",0).edit().putString("background_delivery","periodic").commit();
            secrets.save("disposable-qa-only");context.getSharedPreferences("service_state",0).edit().remove("background_state").commit();
            BackgroundDelivery.schedule(context);
            assertNotNull(context.getSystemService(JobScheduler.class).getPendingJob(BackgroundDelivery.JOB_ID));
            try(ParcelFileDescriptor descriptor=getInstrumentation().getUiAutomation().executeShellCommand("cmd jobscheduler run -f app.valanium.qa "+BackgroundDelivery.JOB_ID);
                FileInputStream input=new FileInputStream(descriptor.getFileDescriptor())){input.readAllBytes();}
            long deadline=System.currentTimeMillis()+200_000;
            while(System.currentTimeMillis()<deadline&&!"scheduled".equals(context.getSharedPreferences("service_state",0).getString("background_state","")))Thread.sleep(250);
            assertEquals("scheduled",context.getSharedPreferences("service_state",0).getString("background_state",""));
            Core restored=ValaniumService.core();assertTrue(restored.isOpen());
            restored.submit(new JSONObject().put("type","history").put("conversation",thread).put("limit",100).toString());
            boolean found=false;deadline=System.currentTimeMillis()+10_000;
            while(System.currentTimeMillis()<deadline){String raw=restored.poll(200);if(raw==null)continue;JSONObject event=new JSONObject(raw);
                if("history".equals(event.optString("type"))){found=event.getJSONArray("messages").toString().contains(offline);break;}}
            assertTrue("A scheduled job must persist an offline message without an Activity or foreground service",found);
            assertFalse(ValaniumService.active());
        }finally{
            BackgroundDelivery.cancel(context);ValaniumService.core().close();
            context.getSharedPreferences("account_session",0).edit().putString("database",oldDatabase).commit();
            context.getSharedPreferences("appearance",0).edit().putString("background_delivery",oldMode).commit();
            if(oldSecret!=null)secrets.save(oldSecret);
        }
    }
    public void testForegroundOnlyModeCancelsScheduledWork() {
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        String old=BackgroundDelivery.mode(context);
        try{context.getSharedPreferences("appearance",0).edit().putString("background_delivery","foreground").commit();BackgroundDelivery.schedule(context);
            assertNull(context.getSystemService(JobScheduler.class).getPendingJob(BackgroundDelivery.JOB_ID));
        }finally{context.getSharedPreferences("appearance",0).edit().putString("background_delivery",old).commit();}
    }
}
