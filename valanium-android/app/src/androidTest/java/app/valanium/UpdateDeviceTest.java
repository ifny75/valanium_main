package app.valanium;

import android.content.Context;
import android.content.pm.ProviderInfo;
import android.net.Uri;
import android.test.InstrumentationTestCase;
import app.valanium.core.Core;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import org.json.JSONObject;

/** Disposable QA package only. Never invokes the installer or accesses user data. */
public final class UpdateDeviceTest extends InstrumentationTestCase {
    public void testProviderOnlyExposesReadOnlyUpdateFiles() throws Exception {
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        String authority=context.getPackageName()+".updates";
        ProviderInfo info=context.getPackageManager().resolveContentProvider(authority,0);
        assertNotNull(info);assertFalse(info.exported);assertTrue(info.grantUriPermissions);
        File root=new File(context.getCacheDir(),"updates");root.mkdirs();
        File file=new File(root,"Valanium-0.0.0.apk");try(FileOutputStream stream=new FileOutputStream(file)){stream.write(new byte[]{1,2,3});}
        Uri uri=new Uri.Builder().scheme("content").authority(authority).appendPath(file.getName()).build();
        try(InputStream stream=context.getContentResolver().openInputStream(uri)){assertEquals(1,stream.read());}
        try{context.getContentResolver().openFileDescriptor(uri,"w");fail("Write access granted");}catch(java.io.FileNotFoundException expected){}
        for(String name:new String[]{"../valanium.db","Valanium-0.0.0.apk/../valanium.db","valanium.db"}){
            try{context.getContentResolver().openInputStream(new Uri.Builder().scheme("content").authority(authority).appendPath(name).build());fail("Unexpected file exposed");}
            catch(java.io.FileNotFoundException expected){}
        }
        file.delete();
    }
    public void testLiveSignedUpdateDownloadsThroughEmbeddedTor() throws Exception {
        Context context=getInstrumentation().getTargetContext();assertEquals("app.valanium.qa",context.getPackageName());
        String proxy=Core.startTor(new File(context.getFilesDir(),"tor-update-qa").getAbsolutePath());assertNotNull(proxy);assertFalse(proxy.isEmpty());
        File directory=new File(context.getCacheDir(),"live-update-qa");
        JSONObject result=new JSONObject(Core.downloadUpdate(true,"0.0.0",directory.getAbsolutePath()));
        assertFalse(result.optString("error"),result.has("error"));assertTrue(result.optBoolean("ready"));
        File file=new File(result.getString("path"));assertEquals(directory.getCanonicalFile(),file.getParentFile().getCanonicalFile());
        assertEquals(result.getLong("bytes"),file.length());assertTrue(file.length()>1000000);
        java.security.MessageDigest hash=java.security.MessageDigest.getInstance("SHA-256");byte[] buffer=new byte[65536];
        try(InputStream stream=new java.io.FileInputStream(file)){int n;while((n=stream.read(buffer))!=-1)hash.update(buffer,0,n);}
        StringBuilder actual=new StringBuilder();for(byte b:hash.digest())actual.append(String.format(java.util.Locale.ROOT,"%02x",b&255));
        assertEquals(result.getString("sha256"),actual.toString());
        // A failed local edit cannot survive the native verifier on the next check.
        try(FileOutputStream stream=new FileOutputStream(file)){stream.write(0);}
        JSONObject repaired=new JSONObject(Core.downloadUpdate(true,"0.0.0",directory.getAbsolutePath()));
        assertTrue(repaired.toString(),repaired.optBoolean("ready"));
        assertEquals(repaired.getLong("bytes"),new File(repaired.getString("path")).length());
    }
}
