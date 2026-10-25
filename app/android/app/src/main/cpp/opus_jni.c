// The whole of the native layer: three calls that hand one 20 ms frame to libopus and take the
// compressed bytes back.
//
// Deliberately thin. Every decision worth making — the bitrate, whether to compress at all, what
// to do when this does not load — is made in Kotlin, where it can be read and tested. There is no
// state here beyond the encoder libopus itself owns.
//
// Buffers are copied in and out rather than pinned with GetPrimitiveArrayCritical: encoding takes
// on the order of a millisecond, and blocking the garbage collector for that long once per frame
// is a worse trade than two small memcpys.

#include <jni.h>
#include <string.h>

#include <opus.h>

// 60 ms at 48 kHz. Earshot sends 20 ms frames, so this is three times what it needs; it exists so
// a future frame size cannot overflow the stack buffer silently.
#define MAX_FRAME_SAMPLES 2880

// The largest a single Opus packet can be.
#define MAX_PACKET_BYTES 1275

JNIEXPORT jlong JNICALL
Java_com_mazlum_earshot_OpusEncoder_nativeCreate(
        JNIEnv *env, jclass clazz, jint rate, jint bitrate, jint expected_loss_percent) {
    (void) env;
    (void) clazz;
    int error = OPUS_OK;
    // VOIP rather than AUDIO: this is a microphone carrying a voice, and the VOIP mode spends its
    // bits accordingly. It is also the mode in which in-band FEC is designed to be used.
    OpusEncoder *encoder = opus_encoder_create(rate, 1, OPUS_APPLICATION_VOIP, &error);
    if (encoder == NULL || error != OPUS_OK) {
        if (encoder != NULL) {
            opus_encoder_destroy(encoder);
        }
        return 0;
    }

    opus_encoder_ctl(encoder, OPUS_SET_BITRATE(bitrate));
    // Redundant copy of the previous frame inside each packet, which is what lets the receiver
    // rebuild one that never arrived. Costs a few kbps and only when the encoder judges it worth
    // spending; `OPUS_SET_PACKET_LOSS_PERC` is what tells it to expect to need it.
    opus_encoder_ctl(encoder, OPUS_SET_INBAND_FEC(1));
    opus_encoder_ctl(encoder, OPUS_SET_PACKET_LOSS_PERC(expected_loss_percent));
    // Complexity 5 of 10. The default is 9, which on a phone is CPU and battery spent for a
    // difference nobody can hear on a voice at this bitrate.
    opus_encoder_ctl(encoder, OPUS_SET_COMPLEXITY(5));
    // Tell it this is speech. It still handles music, this only sets where it starts from.
    opus_encoder_ctl(encoder, OPUS_SET_SIGNAL(OPUS_SIGNAL_VOICE));

    return (jlong) (intptr_t) encoder;
}

/// Returns the number of bytes written into `out` at `out_offset`, or a negative libopus error.
JNIEXPORT jint JNICALL
Java_com_mazlum_earshot_OpusEncoder_nativeEncode(
        JNIEnv *env, jclass clazz, jlong handle, jshortArray pcm, jint samples,
        jbyteArray out, jint out_offset) {
    (void) clazz;
    OpusEncoder *encoder = (OpusEncoder *) (intptr_t) handle;
    if (encoder == NULL || samples <= 0 || samples > MAX_FRAME_SAMPLES) {
        return OPUS_BAD_ARG;
    }
    if ((*env)->GetArrayLength(env, pcm) < samples) {
        return OPUS_BAD_ARG;
    }
    if ((*env)->GetArrayLength(env, out) < out_offset + MAX_PACKET_BYTES) {
        return OPUS_BUFFER_TOO_SMALL;
    }

    opus_int16 frame[MAX_FRAME_SAMPLES];
    (*env)->GetShortArrayRegion(env, pcm, 0, samples, frame);

    unsigned char packet[MAX_PACKET_BYTES];
    int written = opus_encode(encoder, frame, samples, packet, MAX_PACKET_BYTES);
    if (written < 0) {
        return written;
    }
    (*env)->SetByteArrayRegion(env, out, out_offset, written, (const jbyte *) packet);
    return written;
}

JNIEXPORT void JNICALL
Java_com_mazlum_earshot_OpusEncoder_nativeDestroy(JNIEnv *env, jclass clazz, jlong handle) {
    (void) env;
    (void) clazz;
    OpusEncoder *encoder = (OpusEncoder *) (intptr_t) handle;
    if (encoder != NULL) {
        opus_encoder_destroy(encoder);
    }
}

/// The libopus version string, so a build can say out loud what it is carrying.
JNIEXPORT jstring JNICALL
Java_com_mazlum_earshot_OpusEncoder_nativeVersion(JNIEnv *env, jclass clazz) {
    (void) clazz;
    return (*env)->NewStringUTF(env, opus_get_version_string());
}
