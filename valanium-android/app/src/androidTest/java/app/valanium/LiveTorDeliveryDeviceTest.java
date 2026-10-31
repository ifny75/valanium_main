package app.valanium;

import android.content.Context;
import android.test.InstrumentationTestCase;
import app.valanium.core.Core;
import java.io.File;
import java.util.ArrayList;
import java.util.List;
import java.util.function.Predicate;
import org.json.JSONObject;

/** Explicit live-network QA: two disposable accounts, embedded Tor, no user data. */
public final class LiveTorDeliveryDeviceTest extends InstrumentationTestCase {
    private static final class Client implements AutoCloseable {
        final Core core=new Core();
        final List<JSONObject> pending=new ArrayList<>();
        String device;
        Client(File database) throws Exception {
            if(!core.open(database.getAbsolutePath(),"disposable-qa-only"))throw new AssertionError("Cannot open QA database");
        }
        void send(JSONObject command) { if(!core.submit(command.toString()))throw new AssertionError("Command rejected"); }
        JSONObject waitFor(Predicate<JSONObject> predicate) throws Exception {
            for(int i=0;i<pending.size();i++)if(predicate.test(pending.get(i)))return pending.remove(i);
            long deadline=System.currentTimeMillis()+180000;
            while(System.currentTimeMillis()<deadline) {
                String raw=core.poll(200); if(raw==null)continue;
                JSONObject event=new JSONObject(raw);
                if(predicate.test(event))return event;
                if("failed".equals(event.optString("type"))&&!"anonymous_delivery_waiting".equals(event.optString("code")))throw new AssertionError("Live QA failure: "+event.optString("code"));
                if(pending.size()<500)pending.add(event);
            }
            StringBuilder summary=new StringBuilder("Live Tor event deadline exceeded:");
            for(JSONObject e:pending) summary.append(" ").append(e.optString("type")).append("/").append(e.optString("code")).append("/").append(e.optString("state"));
            throw new AssertionError(summary.toString());
        }
        void register() throws Exception {
            send(new JSONObject().put("type","register").put("url","valanium://onion"));
            waitFor(e->"authenticated".equals(e.optString("type")));
            device=waitFor(e->"registered".equals(e.optString("type"))).getString("device");
            // Initial PROFILE precedes startup publication; the second response
            // is from this command, processed after publishing the KeyPackages.
            send(new JSONObject().put("type","profile_get").put("query",device));
            waitFor(e->"profile".equals(e.optString("type"))&&device.equals(e.optString("device")));
            waitFor(e->"profile".equals(e.optString("type"))&&device.equals(e.optString("device")));
        }
        public void close(){core.close();}
    }
    private void exchange(Client from,Client to,String id) throws Exception {
        String body="\u2063OBS1:"+new JSONObject().put("v",1).put("type","text").put("id",id).put("text","Disposable phone QA "+id);
        from.send(new JSONObject().put("type","send").put("recipient_device",to.device).put("body",body));
        from.waitFor(e->"delivery".equals(e.optString("type"))&&id.equals(e.optString("id"))
            &&("accepted".equals(e.optString("state"))||"delivered".equals(e.optString("state"))));
        JSONObject received=to.waitFor(e->"message".equals(e.optString("type"))&&body.equals(e.optString("body")));
        assertEquals(from.device,received.getString("sender_device"));

    }
    public void testPhoneTorDeliveryProfilesAndPairVerification() throws Exception {
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        assertFalse("Embedded Tor did not bootstrap",Core.startTor(new File(context.getFilesDir(),"live-qa-tor").getAbsolutePath()).isEmpty());
        File directory=new File(context.getCacheDir(),"live-qa-"+System.nanoTime());assertTrue(directory.mkdir());
        try(Client alice=new Client(new File(directory,"alice.db"));Client bob=new Client(new File(directory,"bob.db"))) {
            alice.register();bob.register();
            assertFalse("Independent accounts must have different devices",alice.device.equals(bob.device));
            alice.send(new JSONObject().put("type","directory_set").put("device",bob.device).put("standing","approved"));
            bob.send(new JSONObject().put("type","directory_set").put("device",alice.device).put("standing","approved"));
            exchange(alice,bob,"phone-first");exchange(bob,alice,"phone-reply");
            alice.send(new JSONObject().put("type","privacy_set").put("privacy",new JSONObject()
                .put("profile_avatar",new JSONObject().put("scope","approved").put("allow",new org.json.JSONArray()).put("deny",new org.json.JSONArray()))));
            String image="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aX2kAAAAASUVORK5CYII=";
            alice.send(new JSONObject().put("type","profile_set").put("avatar_mime","image/png").put("avatar_base64",image));
            alice.waitFor(e->"profile".equals(e.optString("type"))&&alice.device.equals(e.optString("device"))&&image.equals(e.optString("avatar_base64")));
            bob.send(new JSONObject().put("type","profile_get").put("query",alice.device));
            bob.waitFor(e->"profile".equals(e.optString("type"))&&alice.device.equals(e.optString("device"))&&image.equals(e.optString("avatar_base64")));
            alice.send(new JSONObject().put("type","call_probe").put("recipient_device",bob.device));
            assertEquals(bob.device,alice.waitFor(e->"call_status".equals(e.optString("type"))&&"media_unavailable".equals(e.optString("status"))).getString("peer_device"));
            alice.send(new JSONObject().put("type","profile_get").put("query",bob.device));
            assertEquals(bob.device,alice.waitFor(e->"profile".equals(e.optString("type"))).getString("device"));
            alice.send(new JSONObject().put("type","verify").put("peer_device",bob.device));
            bob.send(new JSONObject().put("type","verify").put("peer_device",alice.device));
            JSONObject a=alice.waitFor(e->"verification".equals(e.optString("type")));
            JSONObject b=bob.waitFor(e->"verification".equals(e.optString("type")));
            assertEquals(a.getString("qr_payload"),b.getString("qr_payload"));
            assertEquals(a.getString("safety_number"),b.getString("safety_number"));
        }
    }
}
