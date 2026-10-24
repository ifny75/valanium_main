# Isolated Android logout test

Build with `./gradlew -PisolatedTest :app:assembleDebug :app:assembleDebugAndroidTest`.
This produces `app.valanium.qa` and `app.valanium.qa.test`, separate from the real
`app.valanium` installation. Install both generated APKs on a test device and run:

```
adb shell am instrument -w -e class app.valanium.LogoutDeviceTest app.valanium.qa.test/android.test.InstrumentationTestRunner
```

The test refuses to run against the real package. It checks the settings actions,
session restart, retained old database, new active database, entry screen, and
that delayed service teardown does not close the new core. Run once with
`POST_NOTIFICATIONS` granted (foreground service), and once denied with the
permission prompt suppressed (activity polling fallback).

This uses an empty local test account; it does not verify server recovery or
transfer of existing chat history. Logout retains the previous encrypted database
but does not automatically import it when an account is recovered.

Rebuild **without** `-PisolatedTest` before distributing or installing the normal
APK. Isolated and normal builds share the same output path.
