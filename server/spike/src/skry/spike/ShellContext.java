package skry.spike;

import android.content.Context;
import android.content.ContextWrapper;
import android.os.Build;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;

/**
 * Provee un {@link Context} válido desde {@code app_process} (uid shell), sin
 * Application real. Necesario para que servicios del sistema (DisplayManager,
 * InputManager, ActivityManager) tengan un contexto con package name.
 *
 * Equivalente standalone al {@code FakeContext}/{@code Workarounds} de scrcpy.
 * Todo por reflection: el spike compila contra android.jar pero estas APIs son
 * internas.
 */
public final class ShellContext {

    /** Package del shell de Android; el AM/IMS lo aceptan como caller legítimo. */
    private static final String SHELL_PACKAGE = "com.android.shell";

    // volatile: init() es synchronized pero get() no; sin volatile otro hilo
    // podría no ver la escritura por falta de barrera de memoria.
    private static volatile Context instance;

    private ShellContext() {}

    /** Inicializa el contexto falso. Idempotente; llamar una vez al arranque. */
    public static synchronized void init() throws Exception {
        if (instance != null) {
            return;
        }
        Class<?> atClass = Class.forName("android.app.ActivityThread");

        // ActivityThread via constructor privado (no hay uno público desde shell).
        Constructor<?> ctor = atClass.getDeclaredConstructor();
        ctor.setAccessible(true);
        Object activityThread = ctor.newInstance();

        // Registrar el singleton interno y marcarlo como system thread. El nombre
        // del campo es estable en AOSP, pero por las dudas (OEM que lo renombre)
        // se cae a buscar el único campo estático de tipo ActivityThread.
        Field current = findFieldByNameOrType(atClass, "sCurrentActivityThread", atClass);
        current.setAccessible(true);
        current.set(null, activityThread);
        setBooleanField(atClass, activityThread, "mSystemThread", true);

        // Samsung One UI (API 31+) puede pedir un ConfigurationController para que
        // getDisplayInfo no explote. Best-effort: si falla, seguimos.
        if (Build.VERSION.SDK_INT >= 31) {
            fillConfigurationController(atClass, activityThread);
        }

        Method getSystemContext = atClass.getDeclaredMethod("getSystemContext");
        getSystemContext.setAccessible(true);
        Context sysCtx = (Context) getSystemContext.invoke(activityThread);

        instance = new ContextWrapper(sysCtx) {
            @Override
            public String getPackageName() {
                return SHELL_PACKAGE;
            }

            @Override
            public String getOpPackageName() {
                return SHELL_PACKAGE;
            }

            @Override
            public Context getApplicationContext() {
                return this;
            }
        };
    }

    public static Context get() {
        if (instance == null) {
            throw new IllegalStateException("ShellContext.init() no fue llamado");
        }
        return instance;
    }

    private static void fillConfigurationController(Class<?> atClass, Object activityThread) {
        try {
            Class<?> ccClass = Class.forName("android.app.ConfigurationController");
            Constructor<?> ccCtor = ccClass.getDeclaredConstructor(atClass);
            ccCtor.setAccessible(true);
            Object cc = ccCtor.newInstance(activityThread);
            Field f = atClass.getDeclaredField("mConfigurationController");
            f.setAccessible(true);
            f.set(activityThread, cc);
        } catch (Exception e) {
            // No fatal: sólo algunos Samsung lo necesitan.
            System.out.println("[skry-nd] ConfigurationController no aplicado: " + e);
        }
    }

    /** Busca un campo por nombre; si no existe, cae al único campo del tipo dado. */
    private static Field findFieldByNameOrType(Class<?> cls, String name, Class<?> type)
            throws NoSuchFieldException {
        try {
            return cls.getDeclaredField(name);
        } catch (NoSuchFieldException e) {
            for (Field f : cls.getDeclaredFields()) {
                if (f.getType() == type) {
                    return f;
                }
            }
            throw e;
        }
    }

    private static void setBooleanField(Class<?> cls, Object target, String name, boolean value)
            throws Exception {
        Field f = cls.getDeclaredField(name);
        f.setAccessible(true);
        f.setBoolean(target, value);
    }
}
