package app.valanium;

import android.app.*;
import android.content.*;
import android.content.pm.ServiceInfo;
import android.media.*;
import android.media.audiofx.*;
import android.os.*;
import org.json.JSONObject;

/** An explicitly accepted call owns this service. No recordings or audio files. */
public final class CallAudioService extends Service implements Events.Listener {
    private static final String CHANNEL="valanium.calls.active", INCOMING="valanium.calls.incoming";
    static final int ALERT=43, ONGOING=44;
    private static volatile CallAudioService instance;
    private static volatile boolean requested;
    private volatile boolean running, muted;
    private boolean speakerWanted;
    private String id="";
    private AudioRecord record;
    private AudioTrack track;
    private AudioManager manager;
    private AudioFocusRequest focus;
    private int oldMode;
    private boolean oldSpeaker;
    private AcousticEchoCanceler echo;
    private NoiseSuppressor noise;
    private Thread capture, playback;
    private PowerManager.WakeLock wake;
    private final java.util.concurrent.ArrayBlockingQueue<short[]> queue=new java.util.concurrent.ArrayBlockingQueue<>(8);
    public static boolean active(){return instance!=null||requested;}
    public static boolean isMuted(){return instance!=null&&instance.muted;}
    @SuppressWarnings("deprecation") public static boolean isSpeaker(){return instance!=null&&(instance.manager==null?instance.speakerWanted:instance.manager.isSpeakerphoneOn());}
    @Override public boolean foregroundInterface(){return false;}
    public static void start(Context c,String id,boolean capture){
        requested=true;
        Intent intent=new Intent(c,CallAudioService.class).putExtra("id",id).putExtra("capture",capture);
        if(Build.VERSION.SDK_INT>=26)c.startForegroundService(intent);else c.startService(intent);
    }
    public static void finish(Context context){requested=false;if(instance!=null)instance.shutdown();context.stopService(new Intent(context,CallAudioService.class));}
    public static void mute(boolean value){CallAudioService service=instance;if(service!=null)service.muted=value;}
    @SuppressWarnings("deprecation") public static void speaker(boolean enabled){CallAudioService s=instance;if(s!=null){s.speakerWanted=enabled;if(s.manager!=null)s.manager.setSpeakerphoneOn(enabled);}}
    private static void send(String type,String id){try{ValaniumService.core().submit(new JSONObject().put("type",type).put("call_id",id).toString());}catch(Exception ignored){}}
    static void channels(Context c){NotificationManager nm=c.getSystemService(NotificationManager.class);
        NotificationChannel live=new NotificationChannel(CHANNEL,I18n.t("Активные звонки"),NotificationManager.IMPORTANCE_LOW);live.setShowBadge(false);nm.createNotificationChannel(live);
        NotificationChannel ring=new NotificationChannel(INCOMING,I18n.t("Входящие звонки"),NotificationManager.IMPORTANCE_HIGH);ring.enableVibration(true);
        ring.setSound(android.provider.Settings.System.DEFAULT_RINGTONE_URI,new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE).build());nm.createNotificationChannel(ring);
    }
    private static PendingIntent open(Context c){return PendingIntent.getActivity(c,44,new Intent(c,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP|Intent.FLAG_ACTIVITY_SINGLE_TOP),PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);}
    static PendingIntent action(Context c,String id,boolean reject){return PendingIntent.getBroadcast(c,reject?43:44,new Intent(c,CallActionReceiver.class).putExtra("id",id).putExtra("reject",reject),PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);}
    static void notifyState(Context context,JSONObject event,boolean foreground){
        if(!"call_status".equals(event.optString("type")))return;
        NotificationManager nm=context.getSystemService(NotificationManager.class);String state=event.optString("status");
        if(!"incoming".equals(state)){nm.cancel(ALERT);return;}if(foreground){nm.cancel(ALERT);return;}channels(context);
        Notification n=new Notification.Builder(context,INCOMING).setSmallIcon(R.drawable.ic_messenger_status).setContentTitle("Valanium")
                .setContentText(I18n.t("Входящий защищённый звонок")).setVisibility(Notification.VISIBILITY_PRIVATE).setCategory(Notification.CATEGORY_CALL)
                .setContentIntent(open(context)).setTimeoutAfter(90000).setAutoCancel(true)
                .addAction(new Notification.Action.Builder(null,I18n.t("Отклонить"),action(context,event.optString("call_id"),true)).build()).build();n.flags|=Notification.FLAG_INSISTENT;nm.notify(ALERT,n);
    }
    private Notification notification(){return new Notification.Builder(this,CHANNEL).setSmallIcon(R.drawable.ic_messenger_status).setContentTitle("Valanium")
            .setContentText(I18n.t("Защищённый аудиозвонок")).setVisibility(Notification.VISIBILITY_PRIVATE).setCategory(Notification.CATEGORY_CALL).setOngoing(true)
            .setContentIntent(open(this)).addAction(new Notification.Action.Builder(null,I18n.t("Завершить"),action(this,id,false)).build()).build();}
    @Override public void onCreate(){super.onCreate();instance=this;channels(this);Events.subscribe(this);
        if(Build.VERSION.SDK_INT>=30)startForeground(ONGOING,notification(),ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE|ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        else if(Build.VERSION.SDK_INT>=29)startForeground(ONGOING,notification(),ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        else startForeground(ONGOING,notification());
        wake=getSystemService(PowerManager.class).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"valanium:audio-call");wake.setReferenceCounted(false);wake.acquire(7_500_000L);
    }
    @Override public int onStartCommand(Intent intent,int flags,int startId){
        if(intent==null){stopSelf();return START_NOT_STICKY;}String next=intent.getStringExtra("id");
        if(next==null||!next.matches("[a-f0-9]{32}")){stopSelf();return START_NOT_STICKY;}
        if(!id.isEmpty()&&!id.equals(next))shutdown();id=next;
        getSystemService(NotificationManager.class).notify(ONGOING,notification());ValaniumService.start(this);
        if(intent.getBooleanExtra("capture",false))audio();return START_NOT_STICKY;
    }
    @SuppressWarnings("deprecation") private void audio(){if(running)return;
        try{
            manager=getSystemService(AudioManager.class);oldMode=manager.getMode();oldSpeaker=manager.isSpeakerphoneOn();
            focus=new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT).setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION).setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build())
                    .setOnAudioFocusChangeListener(value->{if(value<0){send("call_end",id);stopSelf();}}).build();
            if(manager.requestAudioFocus(focus)!=AudioManager.AUDIOFOCUS_REQUEST_GRANTED)throw new IllegalStateException();manager.setMode(AudioManager.MODE_IN_COMMUNICATION);manager.setSpeakerphoneOn(speakerWanted);
            int buffer=Math.max(4096,AudioRecord.getMinBufferSize(16000,AudioFormat.CHANNEL_IN_MONO,AudioFormat.ENCODING_PCM_16BIT));
            record=new AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION,16000,AudioFormat.CHANNEL_IN_MONO,AudioFormat.ENCODING_PCM_16BIT,buffer);
            track=new AudioTrack(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION).setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build(),new AudioFormat.Builder().setSampleRate(16000).setEncoding(AudioFormat.ENCODING_PCM_16BIT).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build(),Math.max(4096,AudioTrack.getMinBufferSize(16000,AudioFormat.CHANNEL_OUT_MONO,AudioFormat.ENCODING_PCM_16BIT)),AudioTrack.MODE_STREAM,AudioManager.AUDIO_SESSION_ID_GENERATE);
            if(record.getState()!=AudioRecord.STATE_INITIALIZED||track.getState()!=AudioTrack.STATE_INITIALIZED)throw new IllegalStateException();
            try{if(AcousticEchoCanceler.isAvailable()){echo=AcousticEchoCanceler.create(record.getAudioSessionId());if(echo!=null)echo.setEnabled(true);}}catch(RuntimeException ignored){}
            try{if(NoiseSuppressor.isAvailable()){noise=NoiseSuppressor.create(record.getAudioSessionId());if(noise!=null)noise.setEnabled(true);}}catch(RuntimeException ignored){}
            running=true;track.play();capture=new Thread(this::captureLoop,"valanium-call-mic");playback=new Thread(this::playbackLoop,"valanium-call-speaker");capture.start();playback.start();
        }catch(Exception error){shutdown();send("call_end",id);stopSelf();}
    }
    private void captureLoop(){short[] pcm=new short[640];int at=0;boolean recording=false;
        try{while(running){
            if(muted){if(recording){record.stop();recording=false;}Thread.sleep(40);at=0;continue;}
            if(!recording){record.startRecording();recording=true;}
            int n=record.read(pcm,at,pcm.length-at,AudioRecord.READ_BLOCKING);if(n<0)throw new IllegalStateException();at+=n;
            if(at==pcm.length){StringBuilder hex=new StringBuilder(2560);for(short sample:pcm){int value=sample&65535;for(int b:new int[]{value&255,value>>>8}){hex.append(Character.forDigit(b>>>4,16));hex.append(Character.forDigit(b&15,16));}}
                if(!muted)ValaniumService.core().submit(new JSONObject().put("type","call_audio").put("call_id",id).put("data",hex.toString()).put("captured_at",System.currentTimeMillis()).toString());at=0;java.util.Arrays.fill(pcm,(short)0);}
        }}catch(Exception error){if(running){send("call_end",id);new Handler(Looper.getMainLooper()).post(this::stopSelf);}}finally{java.util.Arrays.fill(pcm,(short)0);}
    }
    private void playbackLoop(){try{while(running){if(queue.size()<3){Thread.sleep(20);if(!running)return;}
            short[] pcm=queue.poll(100,java.util.concurrent.TimeUnit.MILLISECONDS);if(pcm!=null){if(track.write(pcm,0,pcm.length,AudioTrack.WRITE_BLOCKING)<0)throw new IllegalStateException();java.util.Arrays.fill(pcm,(short)0);}
        }}catch(Exception error){if(running){send("call_end",id);new Handler(Looper.getMainLooper()).post(this::stopSelf);}} }
    @Override public void onEvent(JSONObject event){if(!id.equals(event.optString("call_id")))return;
        if("call_audio".equals(event.optString("type"))){String data=event.optString("data");if(data.length()!=2560)return;short[] pcm=new short[640];try{for(int i=0;i<640;i++){int lo=Integer.parseInt(data.substring(i*4,i*4+2),16),hi=Integer.parseInt(data.substring(i*4+2,i*4+4),16);pcm[i]=(short)(lo|(hi<<8));}if(!queue.offer(pcm)){short[] old=queue.poll();if(old!=null)java.util.Arrays.fill(old,(short)0);queue.offer(pcm);}}catch(RuntimeException ignored){}return;}
        if("call_status".equals(event.optString("type"))){String state=event.optString("status");if("connecting".equals(state)||"active".equals(state))audio();else if(!"ringing".equals(state)&&!"incoming".equals(state))stopSelf();}
    }
    @SuppressWarnings("deprecation") private void shutdown(){running=false;if(record!=null)try{record.stop();}catch(RuntimeException ignored){}if(track!=null)try{track.pause();track.flush();}catch(RuntimeException ignored){}
        for(Thread t:new Thread[]{capture,playback})if(t!=null&&t!=Thread.currentThread())try{t.join(500);}catch(InterruptedException e){Thread.currentThread().interrupt();}
        if(record!=null){record.release();record=null;}if(track!=null){track.release();track=null;}if(echo!=null){echo.release();echo=null;}if(noise!=null){noise.release();noise=null;}
        queue.clear();if(manager!=null){manager.setSpeakerphoneOn(oldSpeaker);manager.setMode(oldMode);if(focus!=null)manager.abandonAudioFocusRequest(focus);manager=null;}
    }
    @Override public void onDestroy(){send("call_end",id);shutdown();Events.unsubscribe(this);if(wake!=null&&wake.isHeld())wake.release();wake=null;instance=null;requested=false;stopForeground(STOP_FOREGROUND_REMOVE);super.onDestroy();}
    @Override public void onTimeout(int startId,int fgsType){stopSelf();}
    @Override public IBinder onBind(Intent intent){return null;}
}
