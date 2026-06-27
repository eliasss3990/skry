package skry.spike;

import android.app.ActivityOptions;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;

/**
 * Lanza contenido en una pantalla virtual independiente (la creada por
 * {@link VirtualDisplayFactory}). Lo que se lance acá corre <i>en ese display</i>
 * y no aparece en el panel físico del teléfono.
 *
 * Usa {@link ActivityOptions#setLaunchDisplayId(int)} (API pública desde 26),
 * así que casi no necesita reflection. El permiso real para lanzar en el display
 * lo da el flag TRUSTED de la pantalla virtual (ver VirtualDisplayFactory).
 */
public final class DisplayLauncher {

    private DisplayLauncher() {}

    /** Lanza el launcher (home) en el display: arranca con el escritorio del celu. */
    public static void launchHome(Context context, int displayId) {
        Intent intent = new Intent(Intent.ACTION_MAIN);
        intent.addCategory(Intent.CATEGORY_HOME);
        intent.setFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        startOnDisplay(context, intent, displayId, "home");
    }

    /** Lanza una app concreta (por package) en el display independiente. */
    public static void launchApp(Context context, int displayId, String packageName) {
        PackageManager pm = context.getPackageManager();
        Intent intent = pm.getLaunchIntentForPackage(packageName);
        if (intent == null) {
            throw new IllegalArgumentException("sin actividad de inicio para package '" + packageName + "'");
        }
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        startOnDisplay(context, intent, displayId, packageName);
    }

    private static void startOnDisplay(Context context, Intent intent, int displayId, String what) {
        ActivityOptions options = ActivityOptions.makeBasic();
        options.setLaunchDisplayId(displayId);
        try {
            context.startActivity(intent, options.toBundle());
            System.out.println("[skry-nd] lanzado '" + what + "' en display " + displayId);
        } catch (Exception e) {
            // Típico si el display no es TRUSTED: el ATM niega lanzar ahí.
            System.out.println("[skry-nd] no se pudo lanzar '" + what + "' en display "
                    + displayId + ": " + e);
            throw e;
        }
    }
}
