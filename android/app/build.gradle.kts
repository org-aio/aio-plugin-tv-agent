plugins {
    alias(libs.plugins.android.application)
}

android {
    namespace = "site.addzero.tvagent"
    compileSdk = libs.versions.android.compileSdk.get().toInt()

    defaultConfig {
        applicationId = "site.addzero.tvagent"
        minSdk = libs.versions.android.minSdk.get().toInt()
        targetSdk = libs.versions.android.targetSdk.get().toInt()
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        buildConfig = false
    }

    sourceSets["main"].assets.directories.add(rootProject.file("../frontend").absolutePath)
}
