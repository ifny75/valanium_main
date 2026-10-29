package app.valanium;

public final class ValaniumApplication extends android.app.Application {
    @Override public void onCreate() {
        super.onCreate();
        I18n.initialize(this);
    }
}
