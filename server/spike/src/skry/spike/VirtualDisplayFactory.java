package skry.spike;

import android.content.Context;
import android.view.Surface;

import java.lang.reflect.Method;

/**
 * Crea una pantalla virtual <b>independiente</b> (no espejo): tiene su propio
 * contenido, lo que se lance ahí no aparece en el panel físico del teléfono.
 * Es la base de la "feature estrella": elegir contenido desde el celu, dejar el
 * celu libre, y seguir transmitiendo ese contenido a la PC.
 *
 * Contrasta con {@link Spike3Main} clásico, que crea un display que <i>espeja</i>
 * la pantalla 0 (flags = 0).
 */
public final class VirtualDisplayFactory {

    // Flags de createVirtualDisplay. Valores estables del framework (algunos @hide,
    // pero el método público acepta el int).
    private static final int FLAG_PUBLIC = 1 << 0;
    private static final int FLAG_PRESENTATION = 1 << 1;
    private static final int FLAG_OWN_CONTENT_ONLY = 1 << 3;
    private static final int FLAG_TRUSTED = 1 << 10;

    /** Display independiente sin TRUSTED (suficiente para mostrar y, en muchos ROM, lanzar). */
    private static final int FLAGS_BASE = FLAG_PUBLIC | FLAG_PRESENTATION | FLAG_OWN_CONTENT_ONLY;

    /** Con TRUSTED: habilita lanzar actividades arbitrarias e inyectar input al display. */
    private static final int FLAGS_TRUSTED = FLAGS_BASE | FLAG_TRUSTED;

    private VirtualDisplayFactory() {}

    /**
     * Crea el display independiente y devuelve el {@code VirtualDisplay}. Intenta
     * primero con TRUSTED (necesario para input/launch en la mayoría de ROMs) y,
     * si el ROM lo rechaza (One UI 8 / Android 16 puede), reintenta sin TRUSTED.
     */
    public static Object createIndependent(Context context, String name, int w, int h, int dpi, Surface surface)
            throws Exception {
        Object dm = context.getSystemService(Context.DISPLAY_SERVICE);
        if (dm == null) {
            throw new IllegalStateException("DISPLAY_SERVICE no disponible desde el contexto shell");
        }
        Method create = dm.getClass().getMethod("createVirtualDisplay",
                String.class, int.class, int.class, int.class, Surface.class, int.class);

        try {
            Object vd = create.invoke(dm, name, w, h, dpi, surface, FLAGS_TRUSTED);
            System.out.println("[skry-nd] display independiente creado con TRUSTED");
            return vd;
        } catch (Exception e) {
            // Algunos ROM (Samsung One UI reciente) rechazan TRUSTED para uid shell.
            // Sin TRUSTED igual se ve; sólo limita lanzar apps no exportadas / input.
            System.out.println("[skry-nd] TRUSTED rechazado (" + rootCause(e) + "); reintento sin TRUSTED");
            Object vd = create.invoke(dm, name, w, h, dpi, surface, FLAGS_BASE);
            System.out.println("[skry-nd] display independiente creado SIN TRUSTED");
            return vd;
        }
    }

    /** ID del display creado (necesario para lanzar apps e inyectar input ahí). */
    public static int getDisplayId(Object virtualDisplay) throws Exception {
        Method getDisplay = virtualDisplay.getClass().getMethod("getDisplay");
        Object display = getDisplay.invoke(virtualDisplay);
        Method getDisplayId = display.getClass().getMethod("getDisplayId");
        return (int) getDisplayId.invoke(display);
    }

    public static void release(Object virtualDisplay) {
        try {
            virtualDisplay.getClass().getMethod("release").invoke(virtualDisplay);
        } catch (Throwable ignored) {
            // best-effort
        }
    }

    private static String rootCause(Throwable t) {
        Throwable c = t;
        while (c.getCause() != null) {
            c = c.getCause();
        }
        return c.toString();
    }
}
