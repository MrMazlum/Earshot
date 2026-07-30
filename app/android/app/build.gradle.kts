plugins {
    id("com.android.application")
    id("kotlin-android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "com.mazlum.earshot"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = JavaVersion.VERSION_17.toString()
    }

    defaultConfig {
        applicationId = "com.mazlum.earshot"
        minSdk = 26  // notification channels + modern foreground services
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    buildTypes {
        release {
            // Signed with the DEBUG key, deliberately and temporarily. It makes `flutter run
            // --release` and sideloading work with no keystore in the tree, and it is why an APK
            // from CI will not install over one you built yourself: different signature.
            //
            // Real signing needs a keystore, and a keystore must never be committed to this
            // repository. Until that exists, the release APK is fine to sideload and is not a
            // basis for trusting the build - SECURITY.md says so too.
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

flutter {
    source = "../.."
}

dependencies {
    // ContextCompat / ActivityCompat for the permission + foreground-service helpers.
    implementation("androidx.core:core-ktx:1.13.1")
}
