import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    id("kotlin-android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// Release signing, if a keystore has been provided. `key.properties` is gitignored in three places
// and holds `storeFile`, `storePassword`, `keyAlias`, `keyPassword`; CI writes it from repository
// secrets, and a local build gets one by following "Signing a release" in CONTRIBUTING.md.
//
// When it is absent the build still works and falls back to the debug key, because requiring a
// keystore to run `flutter build apk --release` would mean nobody could build the app from a fresh
// clone. The difference is visible: `signedWithRealKey` is printed by the CI job and stated in the
// release notes, so a debug-signed artifact is never quietly passed off as a signed one.
val keystoreProperties = Properties()
val keystorePropertiesFile = rootProject.file("key.properties")
if (keystorePropertiesFile.exists()) {
    FileInputStream(keystorePropertiesFile).use { keystoreProperties.load(it) }
}
val signedWithRealKey = keystoreProperties.getProperty("storeFile") != null

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

    signingConfigs {
        if (signedWithRealKey) {
            create("release") {
                storeFile = rootProject.file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            // The real key when there is one, the debug key when there is not.
            //
            // The debug key is not a signature anybody should trust: its password is the published
            // string "android", and the keystore is generated on demand by whichever machine builds
            // - so consecutive CI runs can produce APKs that will not even install over each other.
            // It is fine for sideloading your own build and is not a basis for trusting somebody
            // else's. SECURITY.md says the same thing to anyone downloading a release.
            signingConfig = if (signedWithRealKey) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
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
