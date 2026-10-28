package dev.vieww.audio;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.media.session.MediaSession;
import android.os.Build;
import android.os.IBinder;

/**
 * Keeps the process playing with the screen off (AU-04).
 *
 * Android freezes a backgrounded app within seconds unless it runs a
 * foreground service; for audio that is a service of type mediaPlayback
 * showing a media notification bound to the app's MediaSession, which is
 * also what puts the controls on the lock screen. The platform crate starts
 * it when playback starts and stops it when playback stops.
 *
 * Declared in the manifest by ci/mobile/apk-add-code.sh --background-audio
 * (service, FOREGROUND_SERVICE and FOREGROUND_SERVICE_MEDIA_PLAYBACK).
 */
public final class PlaybackService extends Service {
    private static final String CHANNEL = "vieww_playback";
    private static final int NOTIFICATION_ID = 0x7669;

    private static volatile MediaSession.Token token;
    private static volatile String title = "";
    private static volatile String artist = "";

    /** Start, or refresh the notification of, the service. */
    public static void start(Context context, MediaSession.Token sessionToken,
                             String trackTitle, String trackArtist) {
        token = sessionToken;
        title = trackTitle == null ? "" : trackTitle;
        artist = trackArtist == null ? "" : trackArtist;
        Intent intent = new Intent(context, PlaybackService.class);
        if (Build.VERSION.SDK_INT >= 26) {
            context.startForegroundService(intent);
        } else {
            context.startService(intent);
        }
    }

    /** Stop the service and remove its notification. */
    public static void stop(Context context) {
        context.stopService(new Intent(context, PlaybackService.class));
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        Notification notification = build();
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        } else {
            startForeground(NOTIFICATION_ID, notification);
        }
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        if (Build.VERSION.SDK_INT >= 24) {
            stopForeground(STOP_FOREGROUND_REMOVE);
        } else {
            stopForeground(true);
        }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @SuppressWarnings("deprecation")
    private Notification build() {
        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= 26) {
            NotificationManager manager = getSystemService(NotificationManager.class);
            if (manager != null && manager.getNotificationChannel(CHANNEL) == null) {
                manager.createNotificationChannel(new NotificationChannel(
                        CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW));
            }
            builder = new Notification.Builder(this, CHANNEL);
        } else {
            builder = new Notification.Builder(this);
        }
        Notification.MediaStyle style = new Notification.MediaStyle();
        MediaSession.Token t = token;
        if (t != null) {
            style.setMediaSession(t);
        }
        builder.setSmallIcon(android.R.drawable.ic_media_play)
                .setContentTitle(title)
                .setContentText(artist)
                .setStyle(style)
                .setOngoing(true)
                .setVisibility(Notification.VISIBILITY_PUBLIC);
        Intent launch = getPackageManager().getLaunchIntentForPackage(getPackageName());
        if (launch != null) {
            int piFlags = Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0;
            builder.setContentIntent(PendingIntent.getActivity(this, 0, launch, piFlags));
        }
        return builder.build();
    }
}
