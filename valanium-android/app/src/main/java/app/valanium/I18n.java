package app.valanium;

import android.content.Context;
import android.content.res.Configuration;
import android.content.res.Resources;
import org.json.JSONObject;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;

/** Translates application-owned labels only. Never call this on message or seed content. */
public final class I18n {
    private static final Map<String,String> english = new HashMap<>();
    private static final Map<String,String> russian = new HashMap<>();
    private static boolean en;
    private I18n() {}
    static Locale locale(Context context) {
        String choice = context.getSharedPreferences("appearance", 0).getString("language", "auto");
        String code = "auto".equals(choice) ? Resources.getSystem().getConfiguration().getLocales().get(0).getLanguage() : choice;
        return new Locale("ru".equals(code) ? "ru" : "en");
    }
    static void initialize(Context context) {
        try (java.io.InputStream input = context.getAssets().open("ui-en.json")) {
            java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
            byte[] chunk = new byte[8192]; int count;
            while ((count = input.read(chunk)) >= 0) bytes.write(chunk, 0, count);
            JSONObject values = new JSONObject(bytes.toString("UTF-8"));
            java.util.Iterator<String> keys = values.keys();
            while (keys.hasNext()) { String key = keys.next(), value = values.getString(key); english.put(key,value); russian.put(value,key); }
        } catch (Exception error) { android.util.Log.w("Valanium", "Could not load interface translations"); }
        apply(context);
    }
    static void apply(Context context) {
        Locale selected = locale(context);
        en = "en".equals(selected.getLanguage());
        Locale.setDefault(selected);
        Configuration configuration = new Configuration(context.getResources().getConfiguration());
        configuration.setLocale(selected);
        context.getResources().updateConfiguration(configuration, context.getResources().getDisplayMetrics());
    }
    static Context localizedContext(Context context) {
        Configuration configuration = new Configuration(context.getResources().getConfiguration());
        configuration.setLocale(locale(context));
        return context.createConfigurationContext(configuration);
    }
    public static String coreMessage(String code, String message) {
        if (!en) return message;
        if ("anonymous_delivery_waiting".equals(code)) return "Anonymous delivery is pending. The app will retry through Tor.";
        if ("privacy_pass_exchange_pending".equals(code)) return "Keep both devices connected to exchange contact passes, then try again.";
        return message.matches("(?s).*[А-Яа-яЁё].*") ? "Operation failed. Check the connection and try again." : message;
    }
    public static String t(String value) {
        if (value == null) return "";
        String key = value.replaceAll("\\s+", " ").trim();
        if (!en) return russian.getOrDefault(key, value);
        String result = english.get(key);
        if (result == null) return value;
        int first=0,last=value.length();
        while(first<last && Character.isWhitespace(value.charAt(first))) first++;
        while(last>first && Character.isWhitespace(value.charAt(last-1))) last--;
        return value.substring(0,first)+result+value.substring(last);
    }
}
