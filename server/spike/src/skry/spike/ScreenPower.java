package skry.spike;

import android.os.Build;
import android.os.IBinder;

import java.lang.reflect.Method;

/**
 * Apaga/enciende el panel físico del teléfono vía SurfaceControl (uid shell).
 *
 * En modo new-display el stream sale de una pantalla virtual independiente, así
 * que apagar el panel NO afecta el video pero elimina el mayor consumo de batería
 * (la pantalla encendida). Mismo enfoque que scrcpy --turn-screen-off.
 *
 * Todo por reflection: estas APIs de SurfaceControl son internas y cambian de
 * firma entre versiones de Android; se elige la correcta según SDK_INT.
 */
public final class ScreenPower {

    private static final int POWER_MODE_OFF = 0;
    private static final int POWER_MODE_NORMAL = 2;

    private ScreenPower() {}

    /** Apaga el panel físico. Best-effort: si falla, lo loguea y sigue. */
    public static boolean turnOff() {
        return setMainDisplayPowerMode(POWER_MODE_OFF);
    }

    /** Vuelve a encender el panel físico. */
    public static void restore() {
        setMainDisplayPowerMode(POWER_MODE_NORMAL);
    }

    private static boolean setMainDisplayPowerMode(int mode) {
        try {
            Class<?> sc = Class.forName("android.view.SurfaceControl");
            IBinder token = getMainDisplayToken(sc);
            if (token == null) {
                System.out.println("[skry-nd] sin token de panel; no se cambia el power");
                return false;
            }
            Method setPower = sc.getMethod("setDisplayPowerMode", IBinder.class, int.class);
            setPower.invoke(null, token, mode);
            System.out.println("[skry-nd] panel power mode = " + mode);
            return true;
        } catch (Throwable t) {
            System.out.println("[skry-nd] no se pudo cambiar el power del panel: " + t);
            return false;
        }
    }

    private static IBinder getMainDisplayToken(Class<?> sc) throws Exception {
        if (Build.VERSION.SDK_INT >= 29) {
            // API 29+ (igual que scrcpy): getPhysicalDisplayIds(): long[] +
            // getPhysicalDisplayToken(long). Si el device no lista displays
            // físicos, fallback a getInternalDisplayToken().
            long[] ids = (long[]) sc.getMethod("getPhysicalDisplayIds").invoke(null);
            if (ids != null && ids.length > 0) {
                Method getToken = sc.getMethod("getPhysicalDisplayToken", long.class);
                return (IBinder) getToken.invoke(null, ids[0]);
            }
            return (IBinder) sc.getMethod("getInternalDisplayToken").invoke(null);
        }
        return (IBinder) sc.getMethod("getBuiltInDisplay", int.class).invoke(null, 0);
    }
}
