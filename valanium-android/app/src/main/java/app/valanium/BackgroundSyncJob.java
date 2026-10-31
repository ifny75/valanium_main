package app.valanium;

import android.app.job.*;
import android.os.*;
import org.json.JSONObject;
import app.valanium.core.Core;
import app.valanium.core.Commands;

public final class BackgroundSyncJob extends JobService {
    private volatile boolean cancelled;
    @Override public boolean onStartJob(JobParameters params){
        if(ValaniumService.isSigningOut()||"foreground".equals(BackgroundDelivery.mode(this))||ValaniumService.active()||Events.hasListeners())return false;
        cancelled=false;
        new Thread(()->{
            Core core=ValaniumService.core();
            try{
                long deadline=SystemClock.elapsedRealtime()+150_000;
                BackgroundDelivery.state(this,"checking");
                ValaniumService.createChannels(this);
                if(!BackgroundDelivery.openExisting(this,core)||cancelled)return;
                if(!BackgroundDelivery.connect(this,core,()->!cancelled&&!ValaniumService.isSigningOut()&&!ValaniumService.active()&&!Events.hasListeners()&&SystemClock.elapsedRealtime()<deadline))return;
                boolean drained=false;long nextCheck=0;
                while(!cancelled&&SystemClock.elapsedRealtime()<deadline&&!ValaniumService.active()&&!Events.hasListeners()){
                    if(drained&&SystemClock.elapsedRealtime()>=nextCheck){core.submit("{\"type\":\"diagnostics\"}");nextCheck=SystemClock.elapsedRealtime()+2000;}
                    String raw=core.poll(500);if(raw==null)continue;
                    ValaniumService.notifyIncoming(this,raw);Events.publish(raw);
                    JSONObject event=new JSONObject(raw);String type=event.optString("type");
                    if("queue_done".equals(type))drained=true;
                    if("diagnostics".equals(type)&&drained&&event.getJSONObject("report").optInt("pending_envelopes",1)==0){BackgroundDelivery.state(this,"scheduled");break;}
                    if("failed".equals(type)&&!"anonymous_delivery_waiting".equals(event.optString("code")))break;
                }
            }catch(Exception unavailable){BackgroundDelivery.state(this,"waiting");}
            finally{
                new Handler(Looper.getMainLooper()).post(()->{
                    // UI/service may have taken ownership while this task was running.
                    if(!ValaniumService.active()&&!Events.hasListeners()&&core.isOpen())core.submit(Commands.disconnect());
                    if(!cancelled)jobFinished(params,false);
                });
            }
        },"valanium-background-sync").start();
        return true;
    }
    @Override public boolean onStopJob(JobParameters params){cancelled=true;return true;}
}
