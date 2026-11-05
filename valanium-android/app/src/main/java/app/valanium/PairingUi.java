package app.valanium;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.EditText;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;
import org.json.JSONObject;

/** Foreground-only linking UI. Account secrets never appear in a QR or a label. */
final class PairingUi {
    static final int SCAN = 4783;
    interface Host { void send(String command); String unlock() throws Exception; boolean ready(); }
    private final Activity activity;
    private final Host host;
    private final android.content.SharedPreferences prefs;
    private boolean identity, running, offered;
    private String session = "", qr = "", pendingQr = "";
    private AlertDialog dialog, proposal;
    private TextView status;
    private ImageView image;
    private Button confirm;

    PairingUi(Activity activity, Host host) {
        this.activity = activity; this.host = host;
        prefs = activity.getSharedPreferences("appearance", 0);
    }
    private void send(String type) { command(type, null, null); }
    private void command(String type, String key, Object value) {
        try { JSONObject command = new JSONObject().put("type", type);
            if (key != null) command.put(key, value);
            host.send(command.toString());
        } catch (Exception ignored) { fail(); }
    }
    void status(boolean hasIdentity) {
        identity = hasIdentity;
        if (identity && !pendingQr.isEmpty() && host.ready()) {
            String value = pendingQr; pendingQr = ""; open(); command("pairing_connect", "qr", value);
        }
        if (!running && prefs.getBoolean("pairing_discovery", true)) {
            running = true;
            if (identity) command("pairing_discover", "enabled", true);
            else send("pairing_listen");
        }
    }
    void stop() {
        command("pairing_discover", "enabled", false);
        send("pairing_cancel");
        running = false; offered = false; qr = ""; session = ""; pendingQr = "";
        if (image != null) image.setImageDrawable(null);
        if (proposal != null) { proposal.dismiss(); proposal = null; }
        if (dialog != null) { dialog.setOnDismissListener(null); dialog.dismiss(); dialog = null; }
    }
    ViewGroup controls() {
        LinearLayout controls = new LinearLayout(activity); controls.setOrientation(LinearLayout.VERTICAL);
        Button open = new Button(activity); open.setText(I18n.t("Подключить устройство по QR и Wi-Fi"));
        open.setOnClickListener(v -> open()); controls.addView(open);
        Switch automatic = new Switch(activity); automatic.setText(I18n.t("Находить устройства в локальной сети"));
        automatic.setChecked(prefs.getBoolean("pairing_discovery", true));
        automatic.setOnCheckedChangeListener((b, enabled) -> {
            prefs.edit().putBoolean("pairing_discovery", enabled).apply();
            if (!enabled) stop(); else status(identity);
        }); controls.addView(automatic);
        return controls;
    }
    void open() {
        if (dialog != null) { dialog.show(); return; }
        LinearLayout root = new LinearLayout(activity); root.setOrientation(LinearLayout.VERTICAL);
        int pad = (int)(20 * activity.getResources().getDisplayMetrics().density);
        root.setPadding(pad, pad, pad, pad);
        TextView explanation = new TextView(activity);
        explanation.setText(I18n.t("Оба устройства должны быть в одной Wi-Fi сети. Переносятся чаты, сообщения и вложения. Телефон продолжит работать. Подтвердите одинаковый код на обоих экранах."));
        root.addView(explanation);
        status = new TextView(activity); status.setText(I18n.t("Ожидание сопряжения…")); root.addView(status);
        image = new ImageView(activity); image.setAdjustViewBounds(true); image.setBackgroundColor(Color.WHITE);
        image.setVisibility(android.view.View.GONE); root.addView(image, new LinearLayout.LayoutParams(-1, pad*14));
        confirm = new Button(activity); confirm.setText(I18n.t("Код совпадает — подключить")); confirm.setVisibility(android.view.View.GONE);
        confirm.setOnClickListener(v -> {
            try {
                JSONObject command = new JSONObject().put("type", "pairing_confirm").put("session", session);
                if (identity) {
                    String secret = host.unlock();
                    byte[] bytes = secret.getBytes(java.nio.charset.StandardCharsets.UTF_8);
                    StringBuilder hex = new StringBuilder(); for (byte b : bytes) hex.append(String.format(java.util.Locale.ROOT, "%02x", b & 255));
                    java.util.Arrays.fill(bytes, (byte)0); command.put("unlock", hex.toString());
                }
                host.send(command.toString()); confirm.setEnabled(false);
                status.setText(I18n.t("Ожидаем подтверждение второго устройства…"));
            } catch (Exception error) { status.setText(I18n.t("Разблокируйте приложение и повторите сопряжение.")); }
        }); root.addView(confirm);
        if (identity) {
            Button scan = new Button(activity); scan.setText(I18n.t("Сканировать QR камерой"));
            scan.setOnClickListener(v -> activity.startActivityForResult(new Intent(activity, PairingScannerActivity.class), SCAN));
            root.addView(scan);
            EditText input = new EditText(activity); input.setHint(I18n.t("Код сопряжения с другого устройства")); input.setSingleLine(true); root.addView(input);
            Button connect = new Button(activity); connect.setText(I18n.t("Подключить по коду"));
            connect.setOnClickListener(v -> { String value = input.getText().toString().trim(); input.setText(""); command("pairing_connect", "qr", value); }); root.addView(connect);
        } else {
            Button refresh = new Button(activity); refresh.setText(I18n.t("Создать новый QR")); refresh.setOnClickListener(v -> send("pairing_listen")); root.addView(refresh);
            Button copy = new Button(activity); copy.setText(I18n.t("Копировать код подключения"));
            copy.setOnClickListener(v -> {
                if (qr.isEmpty()) return;
                android.content.ClipData clip = android.content.ClipData.newPlainText(I18n.t("Подключение Valanium"), qr);
                android.os.PersistableBundle extras = new android.os.PersistableBundle();
                extras.putBoolean("android.content.extra.IS_SENSITIVE", true); clip.getDescription().setExtras(extras);
                ((android.content.ClipboardManager)activity.getSystemService(Activity.CLIPBOARD_SERVICE)).setPrimaryClip(clip);
            }); root.addView(copy);
        }
        ThemeSurfaces.apply(root, prefs.getInt("accent_color",0xff9770ff), "light".equals(prefs.getString("theme","dark")));
        ScrollView scroll = new ScrollView(activity); scroll.addView(root);
        dialog = new AlertDialog.Builder(activity).setTitle(I18n.t("Подключение аккаунта"))
                .setView(scroll).setNegativeButton(I18n.t("Закрыть"), null).create();
        dialog.setOnDismissListener(d -> { dialog = null; qr = ""; session = ""; running = false; send("pairing_cancel"); });
        dialog.show(); dialog.getWindow().addFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE);
        if (!identity) { if (qr.isEmpty()) send("pairing_listen"); else renderQr(); }
    }
    private void renderQr() {
        if (image == null || qr.isEmpty()) return;
        try {
            com.google.zxing.common.BitMatrix matrix = VerificationQr.encode(qr);
            int size = matrix.getWidth(); int[] pixels = new int[size*size];
            for (int y=0;y<size;y++) for (int x=0;x<size;x++) pixels[y*size+x] = matrix.get(x,y) ? Color.BLACK : Color.WHITE;
            image.setImageBitmap(Bitmap.createBitmap(pixels,size,size,Bitmap.Config.ARGB_8888));
            image.setVisibility(android.view.View.VISIBLE);
            status.setText(I18n.t("Отсканируйте этот QR на устройстве с аккаунтом. QR действует 3 минуты."));
        } catch (Exception error) { fail(); }
    }
    void event(JSONObject event) {
        if ("pairing_found".equals(event.optString("type"))) {
            if (!identity || offered || !prefs.getBoolean("pairing_discovery", true) || dialog != null) return;
            offered = true;
            proposal = new AlertDialog.Builder(activity).setTitle(I18n.t("Рядом найдено устройство Valanium"))
                    .setMessage(I18n.t("Подключить к нему аккаунт? Понадобится отсканировать QR на втором устройстве и сверить код."))
                    .setPositiveButton(I18n.t("Подключить"), (d,w) -> open()).setNegativeButton(I18n.t("Не сейчас"), null).show();
            return;
        }
        String state = event.optString("state"), id = event.optString("session");
        if ("listening".equals(state)) { session=id; qr=event.optString("qr"); if (dialog != null) renderQr(); return; }
        if ("connecting".equals(state)) { session=id; return; }
        if (!id.equals(session)) return;
        if ("transferring".equals(state)) {
            if (status != null) status.setText(I18n.t("Переносим чаты и сообщения… ") + event.optString("code"));
            if (confirm != null) confirm.setVisibility(android.view.View.GONE);
            if (image != null) image.setVisibility(android.view.View.GONE);
        } else if ("verify".equals(state)) {
            if (dialog == null) open();
            status.setText(I18n.t("Сверьте код: ") + event.optString("code"));
            image.setVisibility(android.view.View.GONE); confirm.setVisibility(android.view.View.VISIBLE); confirm.setEnabled(true);
        } else if ("complete".equals(state)) {
            if (dialog != null) { dialog.setOnDismissListener(null); dialog.dismiss(); dialog=null; }
            if (image != null) image.setImageDrawable(null);
            qr="";session="";running=false;status(identity);
            android.widget.Toast.makeText(activity,I18n.t("Устройство подключено. Чаты и история перенесены."),android.widget.Toast.LENGTH_LONG).show();
        } else if ("failed".equals(state)) fail();
    }
    void result(int code, Intent data) {
        if (code == Activity.RESULT_OK && data != null) {
            pendingQr = data.getStringExtra("pairing_qr");
            if (pendingQr == null) pendingQr = "";
            if (host.ready()) status(identity);
        }
    }
    void fail() {
        qr="";
        if (image != null) image.setImageDrawable(null);
        if (image != null) image.setVisibility(android.view.View.GONE);
        if (confirm != null) confirm.setVisibility(android.view.View.GONE);
        if (status != null) status.setText(I18n.t("Сопряжение не удалось. Проверьте Wi-Fi и создайте новый QR."));
    }
}
