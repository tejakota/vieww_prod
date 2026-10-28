package dev.vieww.audio;

import android.media.AudioManager;

/**
 * Forwards audio-focus changes to vieww-audio (AU-02). The native method is
 * registered by the platform crate (src/audio_android.rs) at startup.
 */
public final class FocusListener implements AudioManager.OnAudioFocusChangeListener {
    @Override
    public void onAudioFocusChange(int focusChange) {
        nativeFocusChange(focusChange);
    }

    static native void nativeFocusChange(int focusChange);
}
