plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

val releaseStore = providers.environmentVariable("DBM_ANDROID_KEYSTORE")
val releaseStorePassword = providers.environmentVariable("DBM_ANDROID_KEYSTORE_PASSWORD")
val releaseKeyAlias = providers.environmentVariable("DBM_ANDROID_KEY_ALIAS")
val releaseKeyPassword = providers.environmentVariable("DBM_ANDROID_KEY_PASSWORD")
val releaseSigningAvailable = listOf(releaseStore, releaseStorePassword, releaseKeyAlias, releaseKeyPassword)
    .all { it.isPresent && it.get().isNotBlank() }
val buildNumber = providers.environmentVariable("DBM_BUILD_NUMBER").map { it.toInt() }.orElse(1)
require(buildNumber.get() in 1..2_100_000_000) { "DBM_BUILD_NUMBER must be a positive Android version code." }

android {
    namespace = "com.dbm.nativeapp"
    compileSdk = 36
    buildToolsVersion = "36.0.0"
    ndkVersion = "27.2.12479018"
    signingConfigs {
        if (releaseSigningAvailable) create("release") {
            storeFile = file(releaseStore.get())
            storePassword = releaseStorePassword.get()
            keyAlias = releaseKeyAlias.get()
            keyPassword = releaseKeyPassword.get()
        }
    }
    defaultConfig {
        applicationId = "com.dbm.nativeapp"
        minSdk = 26
        targetSdk = 36
        versionCode = buildNumber.get()
        versionName = if (buildNumber.get() > 1) "0.1.${buildNumber.get()}" else "0.1.0-dev"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        externalNativeBuild { cmake { cppFlags += "-std=c++17" } }
    }
    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            buildConfigField("boolean", "ALLOW_DEMO", "true")
        }
        release {
            buildConfigField("boolean", "ALLOW_DEMO", "false")
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = if (releaseSigningAvailable) signingConfigs.getByName("release") else null
        }
    }
    buildFeatures { compose = true; buildConfig = true }
    externalNativeBuild { cmake { path = file("src/main/cpp/CMakeLists.txt"); version = "3.22.1" } }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions.jvmTarget = "17"
    packaging.resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    lint.abortOnError = true
}

// Store distribution is separate; never silently assemble an unsigned release.
if (!releaseSigningAvailable) {
    tasks.configureEach {
        if (name in setOf("assembleRelease", "bundleRelease", "packageRelease", "packageReleaseBundle")) doFirst {
            error("All four DBM_ANDROID release-signing variables are required; refusing an unsigned release.")
        }
    }
}

val checkRust by tasks.registering {
    val output = layout.buildDirectory.dir("rust")
    doLast {
        listOf("arm64-v8a", "x86_64").forEach { abi ->
            check(output.get().file("$abi/libdbm_native_bridge.a").asFile.isFile) {
                "Build the Rust archives first with apps/native/android/build.sh."
            }
        }
    }
}
tasks.configureEach {
    if (name.startsWith("configureCMake") || name.startsWith("buildCMake")) dependsOn(checkRust)
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.09.01")
    implementation(composeBom)
    androidTestImplementation(composeBom)
    implementation("androidx.activity:activity-compose:1.11.0")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.4")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.4")
    implementation("androidx.core:core-ktx:1.17.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
