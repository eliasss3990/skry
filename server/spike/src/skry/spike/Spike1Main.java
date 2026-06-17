package skry.spike;

import android.graphics.Bitmap;
import android.graphics.PixelFormat;
import android.media.Image;
import android.media.ImageReader;
import android.os.Build;
import android.view.Surface;

import java.io.FileOutputStream;
import java.lang.reflect.Method;
import java.nio.ByteBuffer;

/**
 * Spike 1: reconocimiento de la API de captura + intento de sacar 1 frame a PNG.
 *
 * Corre via app_process como uid shell, SIN MediaCodec, SIN red. Objetivo:
 *  1. Loguear las firmas reales de createVirtualDisplay / SurfaceControl en este
 *     dispositivo (Android 16 / One UI 8 / API 36) para no adivinar la API.
 *  2. Intentar capturar a /data/local/tmp/skry-frame.png con ImageReader.
 *
 * No usa APIs ocultas en tiempo de compilacion: todo lo hidden va por reflexion,
 * asi compila contra el android.jar publico.
 */
public final class Spike1Main {

    private static final String OUT_PATH = "/data/local/tmp/skry-frame.png";
    private static final String TAG = "[skry-spike1]";

    public static void main(String[] args) {
        log("==== skry Spike 1 ====");
        log("Device: " + Build.MANUFACTURER + " " + Build.MODEL
                + " | Android " + Build.VERSION.RELEASE + " | SDK " + Build.VERSION.SDK_INT);

        // Fase A: reconocimiento de la API (siempre corre, aunque la captura falle).
        dumpMethods("android.hardware.display.DisplayManager", "createVirtualDisplay");
        dumpMethods("android.hardware.display.DisplayManagerGlobal", "createVirtualDisplay");
        dumpMethods("android.hardware.display.DisplayManagerGlobal", "getDisplayInfo");
        dumpMethods("android.view.SurfaceControl", "createDisplay");
        dumpMethods("android.view.SurfaceControl", "createVirtualDisplay");

        // Fase B: intento de captura.
        try {
            captureToPng();
            log("OK: frame escrito en " + OUT_PATH);
        } catch (Throwable t) {
            log("FALLO la captura: " + t);
            t.printStackTrace();
            log("(La fase A de arriba muestra las firmas disponibles para corregir el camino.)");
        }
    }

    /** Loguea todas las sobrecargas de un metodo de una clase del framework. */
    private static void dumpMethods(String className, String methodName) {
        try {
            Class<?> clazz = Class.forName(className);
            log("--- " + className + "#" + methodName + " ---");
            boolean found = false;
            for (Method m : clazz.getDeclaredMethods()) {
                if (m.getName().equals(methodName)) {
                    found = true;
                    StringBuilder sb = new StringBuilder("  ");
                    if (java.lang.reflect.Modifier.isStatic(m.getModifiers())) {
                        sb.append("static ");
                    }
                    sb.append(m.getReturnType().getSimpleName()).append(" ").append(methodName).append("(");
                    Class<?>[] params = m.getParameterTypes();
                    for (int i = 0; i < params.length; i++) {
                        if (i > 0) sb.append(", ");
                        sb.append(params[i].getSimpleName());
                    }
                    sb.append(")");
                    log(sb.toString());
                }
            }
            if (!found) log("  (no existe en esta version)");
        } catch (ClassNotFoundException e) {
            log("  clase no encontrada: " + className);
        } catch (Throwable t) {
            log("  error inspeccionando " + className + ": " + t);
        }
    }

    /**
     * Intento de captura: resuelve tamano del display por reflexion, crea un
     * ImageReader y un virtual display espejo, y vuelca 1 frame a PNG.
     */
    private static void captureToPng() throws Exception {
        int[] size = resolveDisplaySize();
        int width = size[0];
        int height = size[1];
        log("Display logico: " + width + "x" + height);

        ImageReader reader = ImageReader.newInstance(width, height, PixelFormat.RGBA_8888, 2);
        Surface surface = reader.getSurface();

        Object token = createMirrorDisplay("skry-spike", width, height, surface);
        log("Virtual display creado: " + token);

        // Esperar a que el display renderice y tomar 1 frame.
        Image image = null;
        for (int attempt = 0; attempt < 50 && image == null; attempt++) {
            Thread.sleep(100);
            image = reader.acquireLatestImage();
        }
        if (image == null) {
            throw new IllegalStateException("no llego ningun frame del display (5s)");
        }

        writePng(image, width, height);
        image.close();
        reader.close();
    }

    /** Tamano del display fisico via DisplayManagerGlobal.getDisplayInfo(0). */
    private static int[] resolveDisplaySize() throws Exception {
        Class<?> dmgClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
        Object dmg = dmgClass.getMethod("getInstance").invoke(null);
        Object info = dmgClass.getMethod("getDisplayInfo", int.class).invoke(dmg, 0);
        Class<?> infoClass = info.getClass();
        int w = infoClass.getField("logicalWidth").getInt(info);
        int h = infoClass.getField("logicalHeight").getInt(info);
        return new int[]{w, h};
    }

    /**
     * Crea un virtual display espejo del fisico (displayId 0) con la firma
     * estatica de DisplayManager. Si no existe, intenta el fallback de
     * SurfaceControl. Loguea claramente cual camino tomo.
     */
    private static Object createMirrorDisplay(String name, int w, int h, Surface surface) throws Exception {
        Class<?> dmClass = Class.forName("android.hardware.display.DisplayManager");
        try {
            Method m = dmClass.getMethod("createVirtualDisplay",
                    String.class, int.class, int.class, int.class, Surface.class);
            log("Usando DisplayManager.createVirtualDisplay(String,int,int,int,Surface) [mirror]");
            return m.invoke(null, name, w, h, 0, surface);
        } catch (NoSuchMethodException e) {
            log("DisplayManager.createVirtualDisplay estatica no existe; probando SurfaceControl.createDisplay");
            Class<?> scClass = Class.forName("android.view.SurfaceControl");
            Method create = scClass.getMethod("createDisplay", String.class, boolean.class);
            return create.invoke(null, name, false);
        }
    }

    private static void writePng(Image image, int width, int height) throws Exception {
        Image.Plane plane = image.getPlanes()[0];
        ByteBuffer buffer = plane.getBuffer();
        int pixelStride = plane.getPixelStride();
        int rowStride = plane.getRowStride();
        int rowPadding = rowStride - pixelStride * width;

        Bitmap bitmap = Bitmap.createBitmap(
                width + rowPadding / pixelStride, height, Bitmap.Config.ARGB_8888);
        bitmap.copyPixelsFromBuffer(buffer);

        try (FileOutputStream fos = new FileOutputStream(OUT_PATH)) {
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, fos);
        }
        bitmap.recycle();
    }

    private static void log(String msg) {
        System.out.println(TAG + " " + msg);
    }

    private Spike1Main() {
    }
}
