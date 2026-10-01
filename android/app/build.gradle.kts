plugins {
    alias(libs.plugins.android.application)
}

dependencies {
    implementation(libs.androidx.media3.exoplayer)
    implementation(libs.androidx.media3.ui)
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
        buildConfigField(
            "String",
            "AI_ENDPOINT",
            "\"${providers.gradleProperty("tvAgentAiEndpoint").orElse("https://company-ai.addzero.site/v1").get()}\""
        )
        buildConfigField(
            "String",
            "AI_MODEL",
            "\"${providers.gradleProperty("tvAgentAiModel").orElse("cn:fast-model").get()}\""
        )
        buildConfigField(
            "String",
            "AI_KEY",
            "\"${providers.gradleProperty("tvAgentAiKey").orElse(providers.environmentVariable("AIO_TV_AGENT_AI_KEY").orElse("").get()).get()}\""
        )
        buildConfigField(
            "String",
            "TVBOX_CONFIG",
            "\"${providers.gradleProperty("tvAgentTvboxConfig").orElse("https://szyyds.cn/tv/x.json").get()}\""
        )
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        buildConfig = true
    }

    sourceSets["main"].assets.directories.add(rootProject.file("../frontend").absolutePath)
}
