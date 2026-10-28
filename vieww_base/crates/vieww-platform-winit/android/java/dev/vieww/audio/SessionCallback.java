package dev.vieww.audio;

import android.media.session.MediaSession;

/**
 * Lock-screen, notification, headset, car and watch buttons (AU-04). Each
 * becomes a vieww-audio RemoteCommand; the numbering is shared with
 * src/audio_android.rs and must not change on one side only.
 */
public final class SessionCallback extends MediaSession.Callback {
    static final int PLAY = 0;
    static final int PAUSE = 1;
    static final int STOP = 3;
    static final int NEXT = 4;
    static final int PREVIOUS = 5;
    static final int SEEK_TO = 6;
    static final int FAST_FORWARD = 7;
    static final int REWIND = 8;

    /** How far fast-forward and rewind skip, in milliseconds. */
    static final long SKIP_MS = 15_000L;

    @Override public void onPlay() { nativeCommand(PLAY, 0L); }
    @Override public void onPause() { nativeCommand(PAUSE, 0L); }
    @Override public void onStop() { nativeCommand(STOP, 0L); }
    @Override public void onSkipToNext() { nativeCommand(NEXT, 0L); }
    @Override public void onSkipToPrevious() { nativeCommand(PREVIOUS, 0L); }
    @Override public void onSeekTo(long positionMs) { nativeCommand(SEEK_TO, positionMs); }
    @Override public void onFastForward() { nativeCommand(FAST_FORWARD, SKIP_MS); }
    @Override public void onRewind() { nativeCommand(REWIND, SKIP_MS); }

    static native void nativeCommand(int command, long argument);
}
