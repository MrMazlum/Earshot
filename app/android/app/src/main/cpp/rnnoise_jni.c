// Noise suppression: three calls that hand one frame to RNNoise and take the cleaned one back.
//
// RNNoise works in 480-sample frames at 48 kHz — exactly 10 ms — and in floats scaled like 16-bit
// samples rather than to ±1. Both of those are its rules, not ours, which is why the conversion
// and the splitting happen here instead of in Kotlin: a frame handed over at the wrong scale is
// not an error anywhere, it just sounds wrong.
//
// Earshot sends 20 ms frames, so one captured frame is two RNNoise frames. The denoiser carries
// state between them, so they must be fed in order, and a dropped frame is heard as a brief
// artefact rather than as a crash.

#include <jni.h>
#include <string.h>

#include <rnnoise.h>

// 20 ms at 48 kHz, which is one Earshot frame and two RNNoise frames.
#define MAX_SAMPLES 960

JNIEXPORT jlong JNICALL
Java_com_mazlum_earshot_Denoiser_nativeCreate(JNIEnv *env, jclass clazz) {
    (void) env;
    (void) clazz;
    return (jlong) (intptr_t) rnnoise_create(NULL);
}

/// Cleans `samples` in place. Returns the number of samples processed, or a negative number if the
/// frame was not a length RNNoise can take.
JNIEXPORT jint JNICALL
Java_com_mazlum_earshot_Denoiser_nativeProcess(
        JNIEnv *env, jclass clazz, jlong handle, jshortArray pcm, jint samples) {
    (void) clazz;
    DenoiseState *state = (DenoiseState *) (intptr_t) handle;
    const int frame = rnnoise_get_frame_size();
    if (state == NULL || samples <= 0 || samples > MAX_SAMPLES || samples % frame != 0) {
        return -1;
    }

    jshort pcm_in[MAX_SAMPLES];
    (*env)->GetShortArrayRegion(env, pcm, 0, samples, pcm_in);

    // RNNoise wants floats on the 16-bit scale, not on ±1. Handing it ±1 is silent and wrong: it
    // decides almost everything is noise and the output is close to silence.
    float work[MAX_SAMPLES];
    for (int i = 0; i < samples; i++) {
        work[i] = (float) pcm_in[i];
    }

    for (int offset = 0; offset + frame <= samples; offset += frame) {
        rnnoise_process_frame(state, work + offset, work + offset);
    }

    for (int i = 0; i < samples; i++) {
        float v = work[i];
        // RNNoise can overshoot the 16-bit range on a loud frame; clamping is cheaper than the
        // wrap-around crackle that casting would give.
        if (v > 32767.0f) v = 32767.0f;
        if (v < -32768.0f) v = -32768.0f;
        pcm_in[i] = (jshort) v;
    }

    (*env)->SetShortArrayRegion(env, pcm, 0, samples, pcm_in);
    return samples;
}

JNIEXPORT void JNICALL
Java_com_mazlum_earshot_Denoiser_nativeDestroy(JNIEnv *env, jclass clazz, jlong handle) {
    (void) env;
    (void) clazz;
    DenoiseState *state = (DenoiseState *) (intptr_t) handle;
    if (state != NULL) {
        rnnoise_destroy(state);
    }
}

/// The frame size RNNoise insists on, in samples. Kotlin checks its own frame against it rather
/// than assuming 480 stays true forever.
JNIEXPORT jint JNICALL
Java_com_mazlum_earshot_Denoiser_nativeFrameSize(JNIEnv *env, jclass clazz) {
    (void) env;
    (void) clazz;
    return rnnoise_get_frame_size();
}
