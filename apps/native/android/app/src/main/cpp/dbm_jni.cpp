#include <jni.h>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>
#include <dbm_bridge.h>

extern "C" DbmBridgeSession *dbm_bridge_mobile_session_create(const uint8_t *, size_t, char **);

static jbyteArray bytes(JNIEnv *env, const char *text) {
    if (!text) return nullptr;
    const auto size = static_cast<jsize>(strlen(text));
    jbyteArray result = env->NewByteArray(size);
    env->SetByteArrayRegion(result, 0, size, reinterpret_cast<const jbyte *>(text));
    return result;
}

extern "C" JNIEXPORT jlong JNICALL Java_com_dbm_nativeapp_NativeBridge_create(JNIEnv *env, jobject, jbyteArray path) {
    const jsize size = env->GetArrayLength(path);
    std::vector<uint8_t> data(size);
    env->GetByteArrayRegion(path, 0, size, reinterpret_cast<jbyte *>(data.data()));
    char *error = nullptr;
    auto session = dbm_bridge_mobile_session_create(data.data(), data.size(), &error);
    if (!session) {
        std::string message = error ? error : "Unable to create DBM session";
        dbm_bridge_response_free(error);
        jclass type = env->FindClass("java/lang/IllegalStateException");
        env->ThrowNew(type, message.c_str());
    }
    return reinterpret_cast<jlong>(session);
}

extern "C" JNIEXPORT jlong JNICALL Java_com_dbm_nativeapp_NativeBridge_createDemo(JNIEnv *, jobject) {
    return reinterpret_cast<jlong>(dbm_bridge_demo_session_create());
}

extern "C" JNIEXPORT jbyteArray JNICALL Java_com_dbm_nativeapp_NativeBridge_call(JNIEnv *env, jobject, jlong handle, jbyteArray request) {
    const jsize size = env->GetArrayLength(request);
    std::vector<uint8_t> data(size);
    env->GetByteArrayRegion(request, 0, size, reinterpret_cast<jbyte *>(data.data()));
    char *response = dbm_bridge_session_call(reinterpret_cast<DbmBridgeSession *>(handle), data.data(), data.size());
    jbyteArray result = bytes(env, response);
    dbm_bridge_response_free(response);
    return result;
}

extern "C" JNIEXPORT void JNICALL Java_com_dbm_nativeapp_NativeBridge_free(JNIEnv *, jobject, jlong handle) {
    dbm_bridge_session_free(reinterpret_cast<DbmBridgeSession *>(handle));
}
