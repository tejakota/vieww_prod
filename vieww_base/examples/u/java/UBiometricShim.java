package dev.vieww.u;

import android.app.Activity;
import android.content.Context;
import android.hardware.biometrics.BiometricManager;
import android.hardware.biometrics.BiometricPrompt;
import android.hardware.fingerprint.FingerprintManager;
import android.os.Build;
import android.os.CancellationSignal;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * The biometric half of U's lock: the system BiometricPrompt on API 28+,
 * the FingerprintManager path on 24-27, polled from Rust.
 *
 * The same rules the three camera shim follows, for the same reasons
 * (android-activity does not surface Java callbacks, and a native method on
 * a class from a foreign classloader would need RegisterNatives):
 *
 * - No Gradle, no AGP: this file is compiled and dexed by the crate's
 *   build.rs and loaded by Rust through InMemoryDexClassLoader.
 * - Rust polls; Java never calls back. The state below is the whole of the
 *   shim's memory, and one prompt is in flight at a time — a second
 *   `authenticate` while one runs is refused rather than overwriting it.
 * - The old API's fingerprint path has no system sheet, so the prompt is
 *   invisible there; U's own lock screen is the visible prompt on 24-27.
 */
public final class UBiometricShim {
    /** No prompt has run since the process started. */
    public static final int IDLE = 0;
    /** A prompt is on screen. */
    public static final int RUNNING = 1;
    /** The OS accepted the user. */
    public static final int OK = 2;
    /** The user cancelled the prompt (or pressed the negative button). */
    public static final int CANCELLED = 3;
    /** The OS rejected the user, or told us not now. */
    public static final int DENIED = 4;
    /** No hardware, nothing enrolled, or an unusable call. */
    public static final int UNAVAILABLE = 5;
    /** `authenticate` while a prompt was already running. */
    public static final int NOT_READY = 6;

    /** What the last prompt decided, one of the codes above. */
    private static final AtomicInteger STATE = new AtomicInteger(IDLE);

    /** What the last accepted check was worth: 1 strong (fingerprint/iris), 0 weak (face). */
    private static final AtomicInteger STRENGTH = new AtomicInteger(1);

    /** What the prompt currently on screen will be worth if it succeeds. */
    private static final AtomicInteger PROMPT_STRENGTH = new AtomicInteger(1);

    /** The executor callbacks run on — one thread, the app's whole life. */
    private static final ExecutorService EXEC = Executors.newSingleThreadExecutor();

    private UBiometricShim() {}

    /**
     * What the hardware can check: 1 for a strong biometric (fingerprint or
     * iris), 2 for a weak-only one (face), {@link #UNAVAILABLE} for nothing.
     */
    public static int availability(Context context) {
        if (Build.VERSION.SDK_INT >= 28) {
            BiometricManager manager = context.getSystemService(BiometricManager.class);
            if (manager == null) {
                return UNAVAILABLE;
            }
            if (manager.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_WEAK)
                    != BiometricManager.BIOMETRIC_SUCCESS) {
                return UNAVAILABLE;
            }
            return manager.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG)
                    == BiometricManager.BIOMETRIC_SUCCESS ? 1 : 2;
        }
        FingerprintManager fingerprints = context.getSystemService(FingerprintManager.class);
        if (fingerprints == null || !fingerprints.isHardwareDetected()) {
            return UNAVAILABLE;
        }
        if (!fingerprints.hasEnrolledFingerprints()) {
            return UNAVAILABLE;
        }
        return 1;
    }

    /** What the last prompt decided, one of the codes above. */
    public static int state() {
        return STATE.get();
    }

    /** What the last settled check was worth: 1 strong, 0 weak. */
    public static int strength() {
        return STRENGTH.get();
    }

    /** Begin a prompt. Call on the Java main thread, with one not running. */
    public static void authenticate(Activity activity, String title, String subtitle,
            String cancelLabel, boolean allowDeviceCredential) {
        if (STATE.get() == RUNNING) {
            return;
        }
        STATE.set(RUNNING);
        if (Build.VERSION.SDK_INT >= 28) {
            prompt28(activity, title, subtitle, cancelLabel, allowDeviceCredential);
        } else {
            fingerprint(activity);
        }
    }

    /** Reset to no-prompt, so a later prompt can start from IDLE. */
    public static void reset() {
        if (STATE.get() != RUNNING) {
            STATE.set(IDLE);
        }
    }

    private static void settle(int code, int strength) {
        STRENGTH.set(strength);
        STATE.set(code);
    }

    private static void prompt28(Activity activity, String title, String subtitle,
            String cancelLabel, boolean allowDeviceCredential) {
        // The public SDK has no per-result type constants — the platform's
        // own answer to "what can this device check" is the honest strength,
        // recorded when the prompt starts.
        PROMPT_STRENGTH.set(availability(activity) == 1 ? 1 : 0);

        CancellationSignal cancel = new CancellationSignal();
        BiometricPrompt.AuthenticationCallback callback =
                new BiometricPrompt.AuthenticationCallback() {
                    @Override
                    public void onAuthenticationSucceeded(
                            BiometricPrompt.AuthenticationResult result) {
                        settle(OK, PROMPT_STRENGTH.get());
                    }

                    @Override
                    public void onAuthenticationError(int code, CharSequence message) {
                        if (code == BiometricPrompt.BIOMETRIC_ERROR_USER_CANCELED) {
                            // The negative button reports as a user cancel
                            // through the public SDK.
                            settle(CANCELLED, 1);
                        } else if (code == BiometricPrompt.BIOMETRIC_ERROR_NO_BIOMETRICS
                                || code == BiometricPrompt.BIOMETRIC_ERROR_HW_UNAVAILABLE
                                || code == BiometricPrompt.BIOMETRIC_ERROR_HW_NOT_PRESENT) {
                            settle(UNAVAILABLE, 1);
                        } else {
                            settle(DENIED, 1);
                        }
                    }

                    @Override
                    public void onAuthenticationFailed() {
                        settle(DENIED, 1);
                    }
                };

        BiometricPrompt.Builder builder = new BiometricPrompt.Builder(activity)
                .setTitle(title)
                .setNegativeButton(cancelLabel, EXEC, (dialog, which) -> {
                    // The public SDK's builder takes the listener too; the
                    // callback above is what actually settles the state,
                    // and it sees the same cancel.
                });
        if (subtitle != null && !subtitle.isEmpty()) {
            builder.setSubtitle(subtitle);
        }
        if (allowDeviceCredential) {
            if (Build.VERSION.SDK_INT >= 30) {
                // BIOMETRIC_WEAK admits face and fingerprint alike; the
                // device credential is the fallback the plan asks for.
                builder.setAllowedAuthenticators(
                        BiometricManager.Authenticators.BIOMETRIC_WEAK
                                | BiometricManager.Authenticators.DEVICE_CREDENTIAL);
            } else if (Build.VERSION.SDK_INT >= 29) {
                builder.setDeviceCredentialAllowed(true);
            }
            // API 28 has no device-credential fallback; the biometric is
            // the only gate. U's lock screen says so on that API.
        }
        BiometricPrompt prompt = builder.build();
        prompt.authenticate(cancel, EXEC, callback);
    }

    private static void fingerprint(Activity activity) {
        FingerprintManager fingerprints =
                activity.getSystemService(FingerprintManager.class);
        if (fingerprints == null || !fingerprints.isHardwareDetected()
                || !fingerprints.hasEnrolledFingerprints()) {
            settle(UNAVAILABLE, 1);
            return;
        }
        CancellationSignal cancel = new CancellationSignal();
        FingerprintManager.AuthenticationCallback callback =
                new FingerprintManager.AuthenticationCallback() {
                    @Override
                    public void onAuthenticationSucceeded(
                            FingerprintManager.AuthenticationResult result) {
                        settle(OK, 1);
                    }

                    @Override
                    public void onAuthenticationError(int code, CharSequence message) {
                        if (code == FingerprintManager.FINGERPRINT_ERROR_CANCELED) {
                            settle(CANCELLED, 1);
                        } else {
                            settle(DENIED, 1);
                        }
                    }

                    @Override
                    public void onAuthenticationFailed() {
                        settle(DENIED, 1);
                    }
                };
        fingerprints.authenticate(null, cancel, 0, callback, null);
    }
}
