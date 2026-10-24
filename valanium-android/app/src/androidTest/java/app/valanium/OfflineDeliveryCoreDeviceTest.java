package app.valanium;
import android.content.Context;
import android.test.InstrumentationTestCase;
import app.valanium.core.Core;
import java.io.File;
import java.nio.file.Files;
import org.json.JSONArray;
import org.json.JSONObject;

/** Reopens a disposable MLS database through the actual Android JNI bridge. */
public final class OfflineDeliveryCoreDeviceTest extends InstrumentationTestCase {
    private static Core processRestartCore;
    public void testPrepareForForcedProcessRestart() throws Exception {
        Context context=getInstrumentation().getTargetContext();
        assertEquals("app.valanium.qa",context.getPackageName());
        File dir=context.getExternalFilesDir(null);
        JSONObject fixture=new JSONObject(new String(Files.readAllBytes(new File(dir,"offline-qa.json").toPath()),java.nio.charset.StandardCharsets.UTF_8));
        File database=new File(context.getCacheDir(),"process-restart-qa.db");
        assertFalse(database.exists());
        Files.copy(new File(dir,"offline-qa.db").toPath(),database.toPath());
        processRestartCore=new Core();
        assertTrue(processRestartCore.open(database.getAbsolutePath(),"qa-only"));
        String body="\u2063OBS1:"+new JSONObject().put("type","text").put("id","survive-force-stop").put("text","Очередь после остановки процесса");
        assertTrue(processRestartCore.submit(new JSONObject().put("type","send").put("recipient_device",fixture.getString("peer")).put("body",body).toString()));
        assertEquals("queued",waitFor(processRestartCore,"delivery").getString("state"));
        // Deliberately leave JNI open; the host now force-stops only the QA app.
    }
    public void testReopenAfterForcedProcessRestart() throws Exception {
        Context context=getInstrumentation().getTargetContext();
        assertEquals("app.valanium.qa",context.getPackageName());
        File dir=context.getExternalFilesDir(null);
        JSONObject fixture=new JSONObject(new String(Files.readAllBytes(new File(dir,"offline-qa.json").toPath()),java.nio.charset.StandardCharsets.UTF_8));
        File database=new File(context.getCacheDir(),"process-restart-qa.db");
        assertTrue(database.exists());
        Core core=new Core();
        try {
            assertTrue(core.open(database.getAbsolutePath(),"qa-only"));
            assertTrue(core.submit(new JSONObject().put("type","history").put("conversation",fixture.getString("conversation")).put("limit",10).toString()));
            JSONArray messages=waitFor(core,"history").getJSONArray("messages");
            assertEquals(1,messages.length());
            assertTrue(messages.getJSONObject(0).getString("body").contains("survive-force-stop"));
            assertEquals("queued",messages.getJSONObject(0).getString("delivery"));
        } finally {core.close();database.delete();}
    }
    private JSONObject waitFor(Core core,String type) throws Exception {
        long deadline=System.currentTimeMillis()+10000;
        while(System.currentTimeMillis()<deadline){String raw=core.poll(200);if(raw==null)continue;
            JSONObject event=new JSONObject(raw);
            if(type.equals(event.optString("type")))return event;
            if("failed".equals(event.optString("type")))fail(event.toString());
        }
        throw new AssertionError("Missing event "+type);
    }
    public void testOfflineQueueAndVerificationSurviveNativeCoreRestart() throws Exception {
        Context context=getInstrumentation().getTargetContext();
        assertEquals("Refusing real account","app.valanium.qa",context.getPackageName());
        File dir=context.getExternalFilesDir(null);
        JSONObject fixture=new JSONObject(new String(Files.readAllBytes(new File(dir,"offline-qa.json").toPath()), java.nio.charset.StandardCharsets.UTF_8));
        File database=new File(context.getCacheDir(),"offline-restart-"+System.nanoTime()+".db");
        Files.copy(new File(dir,"offline-qa.db").toPath(),database.toPath());
        Core core=new Core();
        try {
            assertTrue(core.open(database.getAbsolutePath(),"qa-only"));
            for(String id:new String[]{"qa-first","qa-second"}) {
                String body="\u2063OBS1:"+new JSONObject().put("type","text").put("id",id).put("text","Тест без сети");
                assertTrue(core.submit(new JSONObject().put("type","send").put("recipient_device",fixture.getString("peer")).put("body",body).toString()));
                assertEquals("queued",waitFor(core,"delivery").getString("state"));
            }
            core.close();
            assertTrue(core.open(database.getAbsolutePath(),"qa-only"));
            assertTrue(core.submit(new JSONObject().put("type","history").put("conversation",fixture.getString("conversation")).put("limit",10).toString()));
            JSONArray history=waitFor(core,"history").getJSONArray("messages");
            assertEquals(2,history.length());
            assertEquals("queued",history.getJSONObject(0).getString("delivery"));
            assertEquals("queued",history.getJSONObject(1).getString("delivery"));
            assertTrue(history.getJSONObject(0).getString("body").contains("qa-second"));
            assertTrue(history.getJSONObject(1).getString("body").contains("qa-first"));
            assertTrue(core.submit(new JSONObject().put("type","verification_confirm").put("peer_device",fixture.getString("peer")).put("proof","wrong").toString()));
            assertFalse(waitFor(core,"verification_checked").getBoolean("verified"));
            assertTrue(core.submit(new JSONObject().put("type","verification_confirm").put("peer_device",fixture.getString("peer")).put("proof",fixture.getString("proof")).toString()));
            assertTrue(waitFor(core,"verification_checked").getBoolean("verified"));
            core.close();
            assertTrue(core.open(database.getAbsolutePath(),"qa-only"));
            assertTrue(core.submit(new JSONObject().put("type","verify").put("peer_device",fixture.getString("peer")).toString()));
            assertTrue(waitFor(core,"verification").getBoolean("verified"));
        } finally { core.close(); database.delete(); }
    }
}
