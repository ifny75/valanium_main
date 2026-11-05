package app.valanium;
import android.Manifest;
import android.app.*;
import android.content.pm.PackageManager;
import android.view.View;
import android.widget.*;
import org.json.JSONObject;

/** Visible consent and controls; the service owns audio across Activity changes. */
final class CallUi {
 static final int PERMISSION=4788;
 interface Host{void send(JSONObject command);void begin(String id,boolean audio);String name(String peer);}
 private final Activity activity;private final Host host;
 private String id="",peer="",state="";private android.media.Ringtone ringtone;
 private AlertDialog dialog;private LinearLayout root;private Runnable permission;
 private long generation;
 CallUi(Activity activity,Host host){this.activity=activity;this.host=host;}
 private void send(String type){try{host.send(new JSONObject().put("type",type).put("call_id",id));}catch(Exception ignored){}}
 private void permit(Runnable run){if(activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)==PackageManager.PERMISSION_GRANTED)run.run();else{permission=run;activity.requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO},PERMISSION);}}
 void permission(boolean granted){Runnable run=permission;permission=null;if(granted){if(run!=null)run.run();else try{host.send(new JSONObject().put("type","call_state"));}catch(Exception ignored){}}else error();}
 void start(String who){if(id.isEmpty()&&CallAudioService.active()){try{host.send(new JSONObject().put("type","call_state"));}catch(Exception ignored){}return;}if(!id.isEmpty()&&java.util.List.of("incoming","ringing","connecting","active").contains(state)){paint();return;}
  peer=who;long requested=++generation;permit(()->{if(generation!=requested)return;id=java.util.UUID.randomUUID().toString().replace("-","");state="ringing";paint();
   try{host.begin(id,false);host.send(new JSONObject().put("type","call_start").put("recipient_device",peer).put("call_id",id));}catch(Exception error){error();}
  });
 }
 void event(JSONObject event){String next=event.optString("call_id"),status=event.optString("status");
  if("idle".equals(status)){if(!id.isEmpty()&&java.util.List.of("ringing","connecting").contains(state))return;if(dialog!=null)dialog.dismiss();id="";CallAudioService.finish(activity);return;}
  if(!java.util.List.of("incoming","ringing","connecting","active","ended","failed","declined","busy","timeout").contains(status))return;
  if(!id.isEmpty()&&!id.equals(next)&&!"incoming".equals(status))return;
  if(!"incoming".equals(status))stopRing();
  if("incoming".equals(status))generation++;
  id=next;peer=event.optString("peer_device");state=status;
  if(java.util.List.of("ended","failed","declined","busy","timeout").contains(state))CallAudioService.finish(activity);
  else if("connecting".equals(state)||"active".equals(state)){try{host.begin(id,true);}catch(Exception e){send("call_end");error();return;}}
  paint();
 }
 private void button(String label,Runnable action){Button b=new Button(activity);b.setText(I18n.t(label));b.setOnClickListener(v->action.run());root.addView(b);}
 private void paint(){
  if(!"incoming".equals(state))stopRing();
  if(activity.isFinishing()||activity.isDestroyed())return;
  if(dialog==null){root=new LinearLayout(activity);root.setOrientation(LinearLayout.VERTICAL);int p=(int)(20*activity.getResources().getDisplayMetrics().density);root.setPadding(p,p,p,p);dialog=new AlertDialog.Builder(activity).setTitle(I18n.t("Аудиозвонок")).setView(root).setCancelable(false).create();}
  root.removeAllViews();TextView name=new TextView(activity);name.setText(host.name(peer));name.setTextSize(22);root.addView(name);
  TextView status=new TextView(activity);String label=switch(state){case "incoming"->"Входящий звонок";case "ringing"->"Ожидание ответа…";case "connecting"->"Соединяем аудиоканал…";case "active"->"Звонок · сквозное шифрование";case "busy"->"У собеседника другой звонок";case "declined"->"Звонок отклонён";case "timeout"->"Нет ответа на звонок";case "failed"->"Соединение звонка потеряно";default->"Звонок завершён";};status.setText(I18n.t(label));root.addView(status);
  TextView privacy=new TextView(activity);privacy.setText(I18n.t("Звук через relay без Tor. Сервер видит IP участников, но не расшифровывает звук."));privacy.setTextSize(12);root.addView(privacy);
  if("incoming".equals(state)){
   activity.getSystemService(android.app.NotificationManager.class).cancel(CallAudioService.ALERT);
   if(ringtone==null)try{ringtone=android.media.RingtoneManager.getRingtone(activity,android.media.RingtoneManager.getDefaultUri(android.media.RingtoneManager.TYPE_RINGTONE));if(android.os.Build.VERSION.SDK_INT>=28)ringtone.setLooping(true);ringtone.play();}catch(Exception ignored){}
   button("Принять",()->{String chosen=id;permit(()->{if(!chosen.equals(id)||!"incoming".equals(state))return;try{host.begin(chosen,true);send("call_accept");}catch(Exception e){error();}});});
   button("Отклонить",()->{send("call_reject");close();});
  }else if(java.util.List.of("ringing","connecting","active").contains(state)){
   button(CallAudioService.isMuted()?"Включить микрофон":"Выключить микрофон",()->{boolean muted=!CallAudioService.isMuted();CallAudioService.mute(muted);try{host.send(new JSONObject().put("type","call_mute").put("call_id",id).put("muted",muted));}catch(Exception ignored){}paint();});
   button(CallAudioService.isSpeaker()?"Выключить громкую связь":"Громкая связь",()->{CallAudioService.speaker(!CallAudioService.isSpeaker());paint();});
   button("Завершить",()->{send("call_end");close();});
  }else button("Закрыть",this::close);
  android.content.SharedPreferences prefs=activity.getSharedPreferences("appearance",0);ThemeSurfaces.apply(root,prefs.getInt("accent_color",0xff9770ff),"light".equals(prefs.getString("theme","dark")));
  dialog.show();
 }
 private void stopRing(){if(ringtone!=null){ringtone.stop();ringtone=null;}}
 void suspend(){if("incoming".equals(state))try{CallAudioService.notifyState(activity,new JSONObject().put("type","call_status").put("call_id",id).put("status",state),false);}catch(Exception ignored){}hide();}
 void reset(){close();}
 void hide(){stopRing();if(dialog!=null){dialog.dismiss();dialog=null;}}
 private void close(){generation++;CallAudioService.finish(activity);hide();id="";state="";}
 void error(){generation++;stopRing();if(!id.isEmpty())send("call_end");state="failed";CallAudioService.finish(activity);paint();}
}
