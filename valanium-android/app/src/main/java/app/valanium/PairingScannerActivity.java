package app.valanium;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.hardware.Camera;
import android.os.Bundle;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.widget.FrameLayout;
import android.widget.TextView;
import com.google.zxing.BinaryBitmap;
import com.google.zxing.PlanarYUVLuminanceSource;
import com.google.zxing.common.HybridBinarizer;
import com.google.zxing.qrcode.QRCodeReader;

/** Camera frames stay in RAM. No image file, network request or external scanner. */
@SuppressWarnings("deprecation")
public final class PairingScannerActivity extends Activity implements SurfaceHolder.Callback {
    private Camera camera;
    private SurfaceView preview;
    private boolean surfaceReady;
    private volatile boolean decoding, finished;
    private final java.util.concurrent.ExecutorService worker = java.util.concurrent.Executors.newSingleThreadExecutor();
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE);
        FrameLayout root = new FrameLayout(this); preview = new SurfaceView(this);
        root.addView(preview); preview.getHolder().addCallback(this);
        TextView hint = new TextView(this); hint.setText(I18n.t("Наведите камеру на QR подключения Valanium. Назад — отмена."));
        hint.setTextColor(android.graphics.Color.WHITE); hint.setBackgroundColor(0xcc101018); hint.setPadding(24,24,24,24);
        root.addView(hint,new FrameLayout.LayoutParams(-1,-2,android.view.Gravity.BOTTOM));setContentView(root);
        if (checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED)
            requestPermissions(new String[]{Manifest.permission.CAMERA}, 1);
    }
    private void startCamera() {
        if (camera != null || !surfaceReady || finished || checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) return;
        try {
            camera=Camera.open(); Camera.Parameters p=camera.getParameters();
            for (Camera.Size size:p.getSupportedPreviewSizes()) if(size.width==640&&size.height==480){p.setPreviewSize(640,480);break;}
            if(p.getSupportedFocusModes().contains(Camera.Parameters.FOCUS_MODE_CONTINUOUS_PICTURE))p.setFocusMode(Camera.Parameters.FOCUS_MODE_CONTINUOUS_PICTURE);
            camera.setParameters(p); camera.setDisplayOrientation(90); camera.setPreviewDisplay(preview.getHolder());
            camera.setPreviewCallback((bytes, active)-> {
                if(decoding||finished||bytes==null)return; decoding=true;
                Camera.Size size=active.getParameters().getPreviewSize(); byte[] copy=bytes.clone();
                worker.execute(()->{
                    try {
                        String raw=new QRCodeReader().decode(new BinaryBitmap(new HybridBinarizer(
                                new PlanarYUVLuminanceSource(copy,size.width,size.height,0,0,size.width,size.height,false)))).getText();
                        if(!finished&&!isFinishing()&&raw.startsWith("valanium-pair:v2:")&&raw.length()<=2048){finished=true;runOnUiThread(()->{setResult(RESULT_OK,new Intent().putExtra("pairing_qr",raw));finish();});}
                    }catch(Exception ignored){} finally {java.util.Arrays.fill(copy,(byte)0);decoding=false;}
                });
            });camera.startPreview();
        }catch(Exception error){release();android.widget.Toast.makeText(this,I18n.t("Камера недоступна. Можно ввести код сопряжения вручную."),android.widget.Toast.LENGTH_LONG).show();finish();}
    }
    private void release(){if(camera!=null){camera.setPreviewCallback(null);camera.stopPreview();camera.release();camera=null;}}
    @Override protected void onPause(){release();super.onPause();}
    @Override protected void onResume(){super.onResume();startCamera();}
    @Override protected void onDestroy(){finished=true;worker.shutdownNow();super.onDestroy();}
    @Override public void surfaceCreated(SurfaceHolder h){surfaceReady=true;startCamera();}
    @Override public void surfaceChanged(SurfaceHolder h,int f,int w,int height){}
    @Override public void surfaceDestroyed(SurfaceHolder h){surfaceReady=false;release();}
    @Override public void onRequestPermissionsResult(int code,String[] permissions,int[] results){super.onRequestPermissionsResult(code,permissions,results);if(code==1){if(results.length>0&&results[0]==PackageManager.PERMISSION_GRANTED)startCamera();else finish();}}
}
