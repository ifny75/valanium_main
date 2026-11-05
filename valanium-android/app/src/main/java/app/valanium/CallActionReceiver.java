package app.valanium;
import android.content.*;
import org.json.JSONObject;
public final class CallActionReceiver extends BroadcastReceiver {
 @Override public void onReceive(Context context,Intent intent){String id=intent.getStringExtra("id");if(id==null||!id.matches("[a-f0-9]{32}"))return;
  try{ValaniumService.core().submit(new JSONObject().put("type",intent.getBooleanExtra("reject",false)?"call_reject":"call_end").put("call_id",id).toString());}catch(Exception ignored){}
  context.getSystemService(android.app.NotificationManager.class).cancel(CallAudioService.ALERT);CallAudioService.finish(context);
 }
}
