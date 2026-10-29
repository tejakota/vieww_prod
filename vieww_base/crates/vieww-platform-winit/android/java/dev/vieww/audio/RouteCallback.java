package dev.vieww.audio;

import android.media.AudioDeviceCallback;
import android.media.AudioDeviceInfo;

/**
 * Output devices added or removed (AU-09): headphones, Bluetooth, USB. The
 * device streams' supervisors look at the default device at once instead
 * of within their one-second poll. Registered by src/audio_android.rs with
 * AudioManager.registerAudioDeviceCallback (API 23).
 */
public final class RouteCallback extends AudioDeviceCallback {
    @Override
    public void onAudioDevicesAdded(AudioDeviceInfo[] added) {
        nativeRoutesChanged();
    }

    @Override
    public void onAudioDevicesRemoved(AudioDeviceInfo[] removed) {
        nativeRoutesChanged();
    }

    static native void nativeRoutesChanged();
}
