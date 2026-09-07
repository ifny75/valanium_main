plugins {
    id("com.android.application")
}

android {
    namespace = "app.valanium"
    compileSdk = 35
    useLibrary("android.test.runner", false)
    useLibrary("android.test.base", false)

    defaultConfig {
        applicationId = "app.valanium"
        // 26 — минимум, где есть каналы уведомлений и нормальный foreground-сервис.
        minSdk = 26
        targetSdk = 35
        versionCode = 22
        versionName = "0.7.0"
        testInstrumentationRunner = "android.test.InstrumentationTestRunner"
        // Device logout tests must never operate on the user's installed account.
        if (providers.gradleProperty("isolatedTest").isPresent) {
            applicationId = "app.valanium.qa"
        }

        ndk {
            // Portable APK предназначен для современных физических телефонов.
            abiFilters += listOf("arm64-v8a")
        }
    }

    // AndroidX не подключается намеренно: активность наследуется от
    // android.app.Activity, и приложению хватает системных классов. Чем меньше
    // зависимостей у мессенджера, тем меньше поверхность supply chain.

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    /*
      Выпуск здесь **не подписывается**, и это не забывчивость.

      Ключ выпуска пришёл на смену отладочному, которым сборки подписывались
      раньше. Смена сделана через `apksigner rotate`: в APK кладётся
      родословная, доказывающая, что старый ключ уполномочил новый, и только
      благодаря ей обновление встаёт поверх уже установленного приложения. Без
      родословной Android откажет, и человеку останется удалить приложение
      вместе со всей перепиской.

      Родословную Gradle выразить не умеет — он подписал бы одним новым ключом.
      Такой APK установится начисто, а поверх старого не встанет, и заметить
      это можно будет только по жалобам людей. Поэтому Gradle отдаёт
      неподписанный APK: он не устанавливается вовсе, то есть ошибка видна
      сразу, а не через неделю.

      Подписывает `deploy/sign-apk.mjs` — он же и проверяет, что в подписи
      оказались оба ключа.

      **Отладочный ключ выбрасывать нельзя.** На Android до 13 подпись
      проверяется по нему: так работает ротация v3.1, это её обычное поведение.
    */
    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    packaging {
        jniLibs {
            // .so уже собран cargo-ndk и сжат — второй раз не надо.
            useLegacyPackaging = false
            // JNI загружается через libvalanium.so; второй cdylib ядра не нужен.
            excludes += "**/libvalanium_core.so"
        }
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}

/**
 * Сборка нативной части. Требует cargo-ndk и NDK:
 *   cargo install cargo-ndk
 *   sdkmanager "ndk;27.2.12479018"
 */
val cargoNdk by tasks.registering(Exec::class) {
    group = "build"
    description = "Собирает valanium-core в jniLibs через cargo-ndk"
    workingDir = file("${projectDir}/../rust")
    isIgnoreExitValue = false

    val output = file("${projectDir}/src/main/jniLibs")
    commandLine(
        if (System.getProperty("os.name").startsWith("Windows")) "cargo.exe" else "cargo",
        "ndk",
        "-t", "arm64-v8a",
        "-o", output.absolutePath,
        "build", "--release",
        // Встроенный Tor: на Android он обязан ехать внутри библиотеки, потому
        // что система запрещает исполнять скачанные файлы из каталога данных.
        "--features", "tor-embedded",
    )
}

tasks.named("preBuild") {
    dependsOn(cargoNdk)
}
