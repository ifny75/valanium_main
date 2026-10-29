package app.valanium;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.widget.Toast;
import app.valanium.core.Core;
import java.io.File;
import java.io.FileInputStream;
import java.security.MessageDigest;
import java.util.concurrent.atomic.AtomicBoolean;
import org.json.JSONObject;

/** Automatic downloads are anonymous in Tor mode; installing always uses Android's confirmation. */
final class AppUpdates {
    static final int INSTALL_PERMISSION_REQUEST=7412;
    private static final long INTERVAL=6*60*60*1000L;
    private static final AtomicBoolean BUSY=new AtomicBoolean();
    private final Activity activity;
    private final Handler handler=new Handler(Looper.getMainLooper());
    private JSONObject pending;
    private boolean visible=true;
    private boolean offered;
    private final Runnable periodic=()->check(false);
    AppUpdates(Activity activity){this.activity=activity;}
    void pause(){visible=false;handler.removeCallbacks(periodic);}
    void resume(){visible=true;if(pending!=null&&!offered)offer();check(false);}
    private void toast(String text){Toast.makeText(activity,I18n.t(text),Toast.LENGTH_LONG).show();}
    void check(boolean manual){
        if(!visible||activity.isFinishing()||activity.isDestroyed())return;
        handler.removeCallbacks(periodic);handler.postDelayed(periodic,INTERVAL);
        SharedPreferences preferences=activity.getSharedPreferences("appearance",Activity.MODE_PRIVATE);
        if(!manual&&System.currentTimeMillis()-preferences.getLong("updates_last_success",0)<INTERVAL)return;
        if(!BUSY.compareAndSet(false,true)){if(manual)toast("Обновление уже проверяется или скачивается");return;}
        if(manual)toast("Проверяем и скачиваем обновления…");
        new Thread(()->{
            JSONObject result=null;
            try{
                boolean onion="onion".equals(preferences.getString("transport","onion"));
                if(onion){String address=Core.startTor(new File(activity.getFilesDir(),"tor").getAbsolutePath());
                    if(address==null||address.isEmpty())throw new java.io.IOException("Tor unavailable");}
                String current=activity.getPackageManager().getPackageInfo(activity.getPackageName(),0).versionName;
                result=new JSONObject(Core.downloadUpdate(onion,current,new File(activity.getCacheDir(),"updates").getAbsolutePath()));
                if(result.has("error"))throw new java.io.IOException("Update unavailable");
                preferences.edit().putLong("updates_last_success",System.currentTimeMillis()).apply();
            }catch(Exception|LinkageError error){result=null;}
            final JSONObject completed=result;
            BUSY.set(false);
            activity.runOnUiThread(()->{
                if(activity.isFinishing()||activity.isDestroyed())return;
                if(completed!=null&&completed.optBoolean("ready"))pending=completed;
                if(!visible)return;
                if(completed==null){if(manual)toast("Не удалось проверить обновления. Повторите позже.");
                    handler.removeCallbacks(periodic);handler.postDelayed(periodic,60000);return;}
                if(completed.optBoolean("ready")){
                    if(manual||!offered)offer();
                }else if(manual)toast("Установлена актуальная версия");
            });
        },"valanium-update-download").start();
    }
    private void offer(){
        offered=true;
        new AlertDialog.Builder(activity).setTitle(I18n.t("Обновление готово")+" · "+pending.optString("version"))
            .setMessage(I18n.t("Новая версия скачана внутри приложения. Подпись и файл проверены. Установить обновление с сохранением аккаунта?"))
            .setPositiveButton(I18n.t("Установить"),(dialog,which)->install())
            .setNegativeButton(I18n.t("Позже"),null).show();
    }
    void install(){
        if(pending==null)return;
        if(!activity.getPackageManager().canRequestPackageInstalls()){
            toast("Разрешите установку обновлений для Valanium");
            activity.startActivityForResult(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:"+activity.getPackageName())),INSTALL_PERMISSION_REQUEST);return;
        }
        try{
            // Recheck on every install, including after returning from system settings.
            File file=new File(pending.getString("path")).getCanonicalFile();
            File root=new File(activity.getCacheDir(),"updates").getCanonicalFile();
            if(!root.equals(file.getParentFile())||file.length()!=pending.getLong("bytes"))throw new java.io.IOException();
            MessageDigest digest=MessageDigest.getInstance("SHA-256");byte[] buffer=new byte[65536];
            try(FileInputStream input=new FileInputStream(file)){int n;while((n=input.read(buffer))!=-1)digest.update(buffer,0,n);}
            StringBuilder hash=new StringBuilder();for(byte b:digest.digest())hash.append(String.format(java.util.Locale.ROOT,"%02x",b&255));
            if(!hash.toString().equals(pending.getString("sha256")))throw new java.io.IOException();
            Uri uri=new Uri.Builder().scheme("content").authority(activity.getPackageName()+".updates").appendPath(file.getName()).build();
            activity.startActivity(new Intent(Intent.ACTION_VIEW).setDataAndType(uri,"application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION));
        }catch(Exception error){toast("Файл обновления повреждён. Проверьте обновления ещё раз.");}
    }
}
