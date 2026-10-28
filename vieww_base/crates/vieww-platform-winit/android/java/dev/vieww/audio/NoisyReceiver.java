package dev.vieww.audio;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.media.AudioManager;

/**
 * ACTION_AUDIO_BECOMING_NOISY: headphones unplugged or Bluetooth gone, so the
 * next sound would come out of the loudspeaker. Registered at runtime, so it
 * needs no manifest entry.
 */
public final class NoisyReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        if (AudioManager.ACTION_AUDIO_BECOMING_NOISY.equals(intent.getAction())) {
            nativeBecomingNoisy();
        }
    }

    static native void nativeBecomingNoisy();
}
