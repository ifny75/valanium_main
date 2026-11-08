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
 interface Host{void send(JSONObject command);void begin(String id,boolean audio);String name(String peer);void avatar(TextView view,String peer);void banner(View view);}
 private static final java.util.List<String> LIVE=java.util.List.of("ringing","connecting","active");
 // Survives a recreated Activity, so the timer does not restart after rotation.
 private static String timedId="";private static long activeSince;
 private boolean minimized;private LinearLayout banner;private TextView bannerTime,statusView;
 private final Runnable ticker=new Runnable(){public void run(){tick();if(!id.isEmpty()&&(minimized||dialog!=null))activity.getWindow().getDecorView().postDelayed(this,1000);}};
 private final Activity activity;private final Host host;
 private String id="",peer="",state="";private android.media.Ringtone ringtone;
 private Dialog dialog;private LinearLayout root;private Runnable permission;
 private long generation;
 CallUi(Activity activity,Host host){this.activity=activity;this.host=host;}
 private void send(String type){try{host.send(new JSONObject().put("type",type).put("call_id",id));}catch(Exception ignored){}}
 private void permit(Runnable run){if(activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)==PackageManager.PERMISSION_GRANTED)run.run();else{permission=run;activity.requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO},PERMISSION);}}
 void permission(boolean granted){Runnable run=permission;permission=null;if(granted){if(run!=null)run.run();else try{host.send(new JSONObject().put("type","call_state"));}catch(Exception ignored){}}else error();}
 void start(String who){if(id.isEmpty()&&CallAudioService.active()){try{host.send(new JSONObject().put("type","call_state"));}catch(Exception ignored){}return;}if(!id.isEmpty()&&java.util.List.of("incoming","ringing","connecting","active").contains(state)){if(minimized)restore();else paint();return;}
  peer=who;long requested=++generation;permit(()->{if(generation!=requested)return;id=java.util.UUID.randomUUID().toString().replace("-","");state="ringing";paint();
   try{host.begin(id,false);host.send(new JSONObject().put("type","call_start").put("recipient_device",peer).put("call_id",id));}catch(Exception error){error();}
  });
 }
 void event(JSONObject event){String next=event.optString("call_id"),status=event.optString("status");
  if("idle".equals(status)){if(!id.isEmpty()&&java.util.List.of("ringing","connecting").contains(state))return;if(dialog!=null)dialog.dismiss();dialog=null;removeBanner();minimized=false;id="";CallAudioService.finish(activity);return;}
  if(!java.util.List.of("incoming","ringing","connecting","active","ended","failed","declined","busy","timeout").contains(status))return;
  if(!id.isEmpty()&&!id.equals(next)&&!"incoming".equals(status))return;
  // We already closed this screen ourselves; the core's final status must not reopen it.
  if(id.isEmpty()&&java.util.List.of("ended","failed","declined","busy","timeout").contains(status))return;
  if(!"incoming".equals(status))stopRing();
  if("incoming".equals(status))generation++;
  id=next;peer=event.optString("peer_device");state=status;
  if("active".equals(state)&&!id.equals(timedId)){timedId=id;activeSince=android.os.SystemClock.elapsedRealtime();}
  if(java.util.List.of("ended","failed","declined","busy","timeout").contains(state))CallAudioService.finish(activity);
  else if("connecting".equals(state)||"active".equals(state)){try{host.begin(id,true);}catch(Exception e){send("call_end");error();return;}}
  paint();
 }
 private static final int NORMAL=0,ACCEPT=1,DANGER=2,ACTIVE=3;
 private int dp(float value){return Math.round(value*activity.getResources().getDisplayMetrics().density);}
 private int accent(){return activity.getSharedPreferences("appearance",0).getInt("accent_color",0xff9770ff);}
 /** Round call control with its caption underneath, like the system dialer. */
 private View control(int icon,String label,int kind,Runnable action){
  LinearLayout cell=new LinearLayout(activity);cell.setOrientation(LinearLayout.VERTICAL);cell.setGravity(android.view.Gravity.CENTER_HORIZONTAL);
  ImageButton b=new ImageButton(activity);b.setImageResource(icon);b.setScaleType(ImageView.ScaleType.CENTER_INSIDE);b.setPadding(dp(18),dp(18),dp(18),dp(18));
  int fill=switch(kind){case ACCEPT->android.graphics.Color.rgb(52,199,89);case DANGER->android.graphics.Color.rgb(235,64,64);case ACTIVE->android.graphics.Color.WHITE;default->android.graphics.Color.argb(46,255,255,255);};
  android.graphics.drawable.GradientDrawable circle=new android.graphics.drawable.GradientDrawable();circle.setShape(android.graphics.drawable.GradientDrawable.OVAL);circle.setColor(fill);b.setBackground(circle);
  b.setImageTintList(android.content.res.ColorStateList.valueOf(kind==ACTIVE?android.graphics.Color.rgb(16,16,16):android.graphics.Color.WHITE));
  b.setContentDescription(I18n.t(label));b.setOnClickListener(v->action.run());
  cell.addView(b,new LinearLayout.LayoutParams(dp(72),dp(72)));
  TextView caption=new TextView(activity);caption.setText(I18n.t(label));caption.setTextColor(android.graphics.Color.argb(220,255,255,255));caption.setTextSize(13);caption.setGravity(android.view.Gravity.CENTER|android.view.Gravity.TOP);caption.setLines(2);
  LinearLayout.LayoutParams cp=new LinearLayout.LayoutParams(dp(96),LinearLayout.LayoutParams.WRAP_CONTENT);cp.topMargin=dp(8);cell.addView(caption,cp);
  return cell;
 }
 private void paint(){
  if(!"incoming".equals(state))stopRing();
  if(activity.isFinishing()||activity.isDestroyed())return;
  if(minimized){
   if(LIVE.contains(state)){if(dialog!=null){dialog.dismiss();dialog=null;}showBanner();return;}
   // Finished while the person was chatting: say so quietly instead of covering the chat.
   removeBanner();minimized=false;
   if(!"failed".equals(state)){Toast.makeText(activity,I18n.t(label(state)),Toast.LENGTH_SHORT).show();close();return;}
  }
  if(dialog==null){
   dialog=new Dialog(activity,android.R.style.Theme_DeviceDefault_NoActionBar_Fullscreen);dialog.setCancelable(false);
   // Back minimizes a live call instead of being swallowed.
   dialog.setOnKeyListener((d,code,key)->{if(code==android.view.KeyEvent.KEYCODE_BACK&&key.getAction()==android.view.KeyEvent.ACTION_UP&&LIVE.contains(state)){minimize();return true;}return false;});
   root=new LinearLayout(activity);root.setOrientation(LinearLayout.VERTICAL);root.setGravity(android.view.Gravity.CENTER_HORIZONTAL);
   FrameLayout frame=new FrameLayout(activity);frame.addView(root,new FrameLayout.LayoutParams(-1,-1));
   dialog.setContentView(frame);
   android.view.Window window=dialog.getWindow();
   if(window!=null)window.setLayout(android.view.ViewGroup.LayoutParams.MATCH_PARENT,android.view.ViewGroup.LayoutParams.MATCH_PARENT);
  }
  int accent=accent();
  root.setBackground(new android.graphics.drawable.GradientDrawable(android.graphics.drawable.GradientDrawable.Orientation.TOP_BOTTOM,
   new int[]{blend(accent,android.graphics.Color.BLACK,.42f),android.graphics.Color.rgb(12,10,16),android.graphics.Color.rgb(6,6,8)}));
  root.setPadding(dp(24),dp(72),dp(24),dp(56));
  root.removeAllViews();
  TextView kind=new TextView(activity);kind.setGravity(android.view.Gravity.CENTER);kind.setText(I18n.t("incoming".equals(state)?"Входящий аудиозвонок":"Аудиозвонок"));kind.setTextColor(android.graphics.Color.argb(200,255,255,255));kind.setTextSize(15);root.addView(kind,new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,LinearLayout.LayoutParams.WRAP_CONTENT));
  TextView avatar=new TextView(activity);avatar.setGravity(android.view.Gravity.CENTER);avatar.setTextColor(android.graphics.Color.WHITE);avatar.setTextSize(44);
  host.avatar(avatar,peer);
  LinearLayout.LayoutParams ap=new LinearLayout.LayoutParams(dp(132),dp(132));ap.topMargin=dp(40);root.addView(avatar,ap);
  TextView name=new TextView(activity);name.setText(host.name(peer));name.setTextColor(android.graphics.Color.WHITE);name.setTextSize(28);name.setGravity(android.view.Gravity.CENTER);
  name.setTypeface(android.graphics.Typeface.create("sans-serif-medium",android.graphics.Typeface.NORMAL));name.setSingleLine(true);name.setEllipsize(android.text.TextUtils.TruncateAt.END);
  LinearLayout.LayoutParams np=new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,LinearLayout.LayoutParams.WRAP_CONTENT);np.topMargin=dp(20);root.addView(name,np);
  TextView status=new TextView(activity);statusView=status;status.setFontFeatureSettings("tnum");
  status.setText(I18n.t(label(state)));status.setTextColor(android.graphics.Color.argb(190,255,255,255));status.setTextSize(15);status.setGravity(android.view.Gravity.CENTER);
  LinearLayout.LayoutParams sp=new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,LinearLayout.LayoutParams.WRAP_CONTENT);sp.topMargin=dp(6);root.addView(status,sp);
  if(LIVE.contains(state)){
   // The other side's voice; the bars stay low and still while nobody speaks.
   VoiceBars voice=new VoiceBars(activity,CallAudioService::peerLevel,"active".equals(state)?accentOnDark(accent):android.graphics.Color.argb(90,255,255,255));
   LinearLayout.LayoutParams vp=new LinearLayout.LayoutParams(dp(58),dp(34));vp.topMargin=dp(18);vp.gravity=android.view.Gravity.CENTER_HORIZONTAL;root.addView(voice,vp);
  }
  View spring=new View(activity);root.addView(spring,new LinearLayout.LayoutParams(1,0,1f));
  TextView privacy=new TextView(activity);privacy.setText(I18n.t("Звук через relay без Tor. Сервер видит IP участников, но не расшифровывает звук."));
  privacy.setTextColor(android.graphics.Color.argb(140,255,255,255));privacy.setTextSize(12);privacy.setGravity(android.view.Gravity.CENTER);
  LinearLayout.LayoutParams pp=new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,LinearLayout.LayoutParams.WRAP_CONTENT);pp.bottomMargin=dp(28);root.addView(privacy,pp);
  LinearLayout row=new LinearLayout(activity);row.setOrientation(LinearLayout.HORIZONTAL);row.setGravity(android.view.Gravity.CENTER);
  if("incoming".equals(state)){
   activity.getSystemService(android.app.NotificationManager.class).cancel(CallAudioService.ALERT);
   if(ringtone==null)try{ringtone=android.media.RingtoneManager.getRingtone(activity,android.media.RingtoneManager.getDefaultUri(android.media.RingtoneManager.TYPE_RINGTONE));if(android.os.Build.VERSION.SDK_INT>=28)ringtone.setLooping(true);ringtone.play();}catch(Exception ignored){}
   addControl(row,control(R.drawable.ic_call_end,"Отклонить",DANGER,()->{send("call_reject");close();}));
   addControl(row,control(R.drawable.ic_call,"Принять",ACCEPT,()->{String chosen=id;permit(()->{if(!chosen.equals(id)||!"incoming".equals(state))return;try{host.begin(chosen,true);send("call_accept");}catch(Exception e){error();}});}));
  }else if(java.util.List.of("ringing","connecting","active").contains(state)){
   boolean muted=CallAudioService.isMuted(),speaker=CallAudioService.isSpeaker();
   addControl(row,control(muted?R.drawable.ic_mic_off:R.drawable.ic_mic,muted?"Микрофон выключен":"Микрофон",muted?ACTIVE:NORMAL,()->{boolean next=!CallAudioService.isMuted();CallAudioService.mute(next);try{host.send(new JSONObject().put("type","call_mute").put("call_id",id).put("muted",next));}catch(Exception ignored){}paint();}));
   addControl(row,control(R.drawable.ic_call_end,"Завершить",DANGER,()->{send("call_end");close();}));
   addControl(row,control(R.drawable.ic_speaker,"Динамик",speaker?ACTIVE:NORMAL,()->{CallAudioService.speaker(!CallAudioService.isSpeaker());paint();}));
  }else{
   addControl(row,control(R.drawable.ic_call_end,"Закрыть",NORMAL,this::close));
   // The other side hung up: show the outcome briefly, then get out of the way.
   long shown=generation;String finished=id;
   root.postDelayed(()->{if(generation==shown&&finished.equals(id)&&java.util.List.of("ended","declined","busy","timeout").contains(state))close();},2200);
  }
  root.addView(row,new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,LinearLayout.LayoutParams.WRAP_CONTENT));
  FrameLayout frame=(FrameLayout)root.getParent();
  for(int i=frame.getChildCount()-1;i>0;i--)frame.removeViewAt(i);
  if(LIVE.contains(state)){
   ImageButton minimize=new ImageButton(activity);minimize.setImageResource(R.drawable.ic_chevron);minimize.setRotation(90);
   android.graphics.drawable.GradientDrawable glass=new android.graphics.drawable.GradientDrawable();glass.setShape(android.graphics.drawable.GradientDrawable.OVAL);glass.setColor(android.graphics.Color.argb(40,255,255,255));minimize.setBackground(glass);
   minimize.setPadding(dp(12),dp(12),dp(12),dp(12));minimize.setScaleType(ImageView.ScaleType.FIT_CENTER);
   minimize.setImageTintList(android.content.res.ColorStateList.valueOf(android.graphics.Color.WHITE));
   minimize.setContentDescription(I18n.t("Свернуть звонок"));minimize.setOnClickListener(v->minimize());
   FrameLayout.LayoutParams mp=new FrameLayout.LayoutParams(dp(46),dp(46));mp.leftMargin=dp(18);mp.topMargin=dp(40);frame.addView(minimize,mp);
  }
  dialog.show();
  tick();restartTicker();
 }
 private static String label(String state){return switch(state){case "incoming"->"Входящий звонок";case "ringing"->"Ожидание ответа…";case "connecting"->"Соединяем аудиоканал…";case "active"->"Звонок · сквозное шифрование";case "busy"->"У собеседника другой звонок";case "declined"->"Звонок отклонён";case "timeout"->"Нет ответа на звонок";case "failed"->"Соединение звонка потеряно";default->"Звонок завершён";};}
 private static String elapsed(){long s=Math.max(0,(android.os.SystemClock.elapsedRealtime()-activeSince)/1000);return s>=3600?String.format(java.util.Locale.ROOT,"%d:%02d:%02d",s/3600,s/60%60,s%60):String.format(java.util.Locale.ROOT,"%02d:%02d",s/60,s%60);}
 private void tick(){
  boolean timed="active".equals(state)&&id.equals(timedId);
  if(statusView!=null&&dialog!=null)statusView.setText(timed?elapsed()+" · "+I18n.t("Сквозное шифрование"):I18n.t(label(state)));
  if(bannerTime!=null)bannerTime.setText(timed?elapsed():I18n.t(label(state)));
 }
 private void restartTicker(){View decor=activity.getWindow().getDecorView();decor.removeCallbacks(ticker);decor.postDelayed(ticker,1000);}
 private static int accentOnDark(int accent){float[] hsv=new float[3];android.graphics.Color.colorToHSV(accent,hsv);hsv[2]=Math.max(hsv[2],.85f);hsv[1]=Math.min(hsv[1],.65f);return android.graphics.Color.HSVToColor(hsv);}
 /** Leave the call running and go back to the chats; a bar on top brings it back. */
 private void minimize(){minimized=true;if(dialog!=null){dialog.dismiss();dialog=null;}showBanner();}
 void restore(){if(!minimized)return;minimized=false;removeBanner();paint();}
 private void showBanner(){
  if(banner==null){
   banner=new LinearLayout(activity);banner.setOrientation(LinearLayout.HORIZONTAL);banner.setGravity(android.view.Gravity.CENTER_VERTICAL);
   banner.setPadding(dp(16),0,dp(12),0);banner.setClickable(true);banner.setFocusable(true);banner.setOnClickListener(v->restore());
   VoiceBars bars=new VoiceBars(activity,()->Math.max(CallAudioService.peerLevel(),CallAudioService.ownLevel()),android.graphics.Color.WHITE);
   banner.addView(bars,new LinearLayout.LayoutParams(dp(26),dp(18)));
   TextView who=new TextView(activity);who.setTextColor(android.graphics.Color.WHITE);who.setTextSize(14);who.setSingleLine(true);who.setEllipsize(android.text.TextUtils.TruncateAt.END);
   who.setTypeface(android.graphics.Typeface.create("sans-serif-medium",android.graphics.Typeface.NORMAL));who.setTag("who");
   LinearLayout.LayoutParams wp=new LinearLayout.LayoutParams(0,LinearLayout.LayoutParams.WRAP_CONTENT,1f);wp.leftMargin=dp(12);banner.addView(who,wp);
   bannerTime=new TextView(activity);bannerTime.setTextColor(android.graphics.Color.argb(230,255,255,255));bannerTime.setTextSize(13);bannerTime.setFontFeatureSettings("tnum");
   banner.addView(bannerTime,new LinearLayout.LayoutParams(LinearLayout.LayoutParams.WRAP_CONTENT,LinearLayout.LayoutParams.WRAP_CONTENT));
   ImageView open=new ImageView(activity);open.setImageResource(R.drawable.ic_chevron);open.setRotation(-90);open.setImageTintList(android.content.res.ColorStateList.valueOf(android.graphics.Color.WHITE));
   LinearLayout.LayoutParams op=new LinearLayout.LayoutParams(dp(18),dp(18));op.leftMargin=dp(8);banner.addView(open,op);
   host.banner(banner);
  }
  int accent=accent();
  banner.setBackground(new android.graphics.drawable.GradientDrawable(android.graphics.drawable.GradientDrawable.Orientation.LEFT_RIGHT,new int[]{blend(accent,android.graphics.Color.BLACK,.78f),blend(accent,android.graphics.Color.BLACK,.55f)}));
  ((TextView)banner.findViewWithTag("who")).setText(host.name(peer));
  banner.setContentDescription(I18n.t("Вернуться к звонку")+": "+host.name(peer));
  tick();restartTicker();
 }
 private void removeBanner(){if(banner!=null){host.banner(null);banner=null;bannerTime=null;}}
 private void addControl(LinearLayout row,View control){
  LinearLayout.LayoutParams lp=new LinearLayout.LayoutParams(0,LinearLayout.LayoutParams.WRAP_CONTENT,1f);row.addView(control,lp);
 }
 private static int blend(int color,int onto,float amount){
  return android.graphics.Color.rgb(Math.round(android.graphics.Color.red(color)*amount+android.graphics.Color.red(onto)*(1-amount)),
   Math.round(android.graphics.Color.green(color)*amount+android.graphics.Color.green(onto)*(1-amount)),
   Math.round(android.graphics.Color.blue(color)*amount+android.graphics.Color.blue(onto)*(1-amount)));
 }
 private void stopRing(){if(ringtone!=null){ringtone.stop();ringtone=null;}}
 void suspend(){if("incoming".equals(state))try{CallAudioService.notifyState(activity,new JSONObject().put("type","call_status").put("call_id",id).put("status",state),false);}catch(Exception ignored){}hide();}
 void reset(){close();}
 void hide(){stopRing();if(dialog!=null){dialog.dismiss();dialog=null;}}
 private void close(){generation++;CallAudioService.finish(activity);hide();removeBanner();minimized=false;id="";state="";}
 void error(){generation++;stopRing();if(!id.isEmpty())send("call_end");state="failed";CallAudioService.finish(activity);paint();}
}
