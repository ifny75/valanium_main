package app.valanium;

import android.app.job.*;
import android.content.*;
import java.io.File;
import app.valanium.core.Core;
import app.valanium.core.Commands;

/** OS-scheduled fallback, independent of Activity and foreground-service lifetime. */
final class BackgroundDelivery {
    static final int JOB_ID=76015;
    static String mode(Context context){return context.getSharedPreferences("appearance",0).getString("background_delivery","persistent");}
    static boolean persistent(Context context){return "persistent".equals(mode(context));}
    static void schedule(Context context){
        JobScheduler scheduler=context.getSystemService(JobScheduler.class);if(scheduler==null)return;
        if("foreground".equals(mode(context))){scheduler.cancel(JOB_ID);return;}
        JobInfo existing=scheduler.getPendingJob(JOB_ID);if(existing!=null)return;
        scheduler.schedule(new JobInfo.Builder(JOB_ID,new ComponentName(context,BackgroundSyncJob.class))
            .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY).setPersisted(true)
            .setPeriodic(15*60_000L,5*60_000L).build());
    }
    static void cancel(Context context){JobScheduler scheduler=context.getSystemService(JobScheduler.class);if(scheduler!=null)scheduler.cancel(JOB_ID);}
    static void state(Context context,String value){context.getSharedPreferences("service_state",0).edit().putString("background_state",value).putLong("last_attempt",System.currentTimeMillis()).apply();}
    static boolean openExisting(Context context,Core core)throws Exception{
        if(ValaniumService.isSigningOut())return false;
        if(core.isOpen())return true;
        LocalSecretStore secrets=new LocalSecretStore(context);
        // A scheduled task must never satisfy or bypass the owner's app lock.
        if(secrets.locked()){state(context,"unlock_required");return false;}
        String name=context.getSharedPreferences("account_session",0).getString("database","valanium.db");
        if(name==null||name.contains("/")||name.contains("\\"))return false;
        File database=new File(context.getFilesDir(),name);if(!database.isFile())return false;
        String secret=secrets.load();return secret!=null&&core.open(database.getAbsolutePath(),secret);
    }
    static boolean connect(Context context,Core core){
        return connect(context,core,()->!ValaniumService.isSigningOut());
    }
    static boolean connect(Context context,Core core,java.util.function.BooleanSupplier allowed){
        if(!allowed.getAsBoolean())return false;
        String mode=context.getSharedPreferences("appearance",0).getString("transport","onion");
        if("onion".equals(mode)){
            if(Core.startTor(new File(context.getFilesDir(),"tor").getAbsolutePath()).isEmpty())return false;
            if(!allowed.getAsBoolean())return false;
            return core.submit(Commands.connect("valanium://onion"));
        }
        return allowed.getAsBoolean()&&core.submit(Commands.connect("valanium://auto"));
    }
}
