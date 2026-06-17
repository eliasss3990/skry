package skry.spike;

import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaFormat;
import android.view.Surface;

import java.io.FileOutputStream;
import java.lang.reflect.Method;
import java.nio.ByteBuffer;

/**
 * Spike 2: encode del display con MediaCodec a un archivo elementary stream.
 *
 * Reusa el camino de captura validado en el Spike 1 (createVirtualDisplay
 * estática, espejo por default), pero conecta la Surface de un encoder de
 * hardware en vez de un ImageReader. Vuelca ~3 s de H.265 (o H.264 fallback) a
 * /data/local/tmp/skry-out.h26x, que se baja con adb pull y se abre con ffplay.
 * Aísla el encoder (R8: puede dar negro aun con captura OK) sin red.
 */
public final class Spike2Main {

    private static final String TAG = "[skry-spike2]";
    private static final int DURATION_MS = 3000;
    private static final int BITRATE = 8_000_000;
    private static final int FRAME_RATE = 60;

    public static void main(String[] args) {
        log("==== skry Spike 2 (encode) ====");
        try {
            run();
        } catch (Throwable t) {
            log("FALLO: " + t);
            t.printStackTrace();
        }
    }

    private static void run() throws Exception {
        int[] size = resolveDisplaySize();
        int width = size[0];
        int height = size[1];
        log("Display: " + width + "x" + height);

        // Preferir H.265 (HEVC); si no hay encoder, caer a H.264 (AVC).
        String mime = MediaFormat.MIMETYPE_VIDEO_HEVC;
        String outPath = "/data/local/tmp/skry-out.h265";
        MediaCodec codec;
        try {
            codec = MediaCodec.createEncoderByType(mime);
            log("Encoder H.265: " + codec.getName());
        } catch (Exception e) {
            mime = MediaFormat.MIMETYPE_VIDEO_AVC;
            outPath = "/data/local/tmp/skry-out.h264";
            codec = MediaCodec.createEncoderByType(mime);
            log("H.265 no disponible; usando H.264: " + codec.getName());
        }

        MediaFormat format = MediaFormat.createVideoFormat(mime, width, height);
        format.setInteger(MediaFormat.KEY_COLOR_FORMAT,
                MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface);
        format.setInteger(MediaFormat.KEY_BIT_RATE, BITRATE);
        format.setInteger(MediaFormat.KEY_FRAME_RATE, FRAME_RATE);
        format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1);

        codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE);
        Surface inputSurface = codec.createInputSurface();
        codec.start();

        Object vd = createMirrorDisplay("skry-spike2", width, height, inputSurface);
        log("Virtual display + encoder conectados. Capturando " + DURATION_MS + " ms...");

        long bytesWritten = 0;
        int frames = 0;
        MediaCodec.BufferInfo info = new MediaCodec.BufferInfo();
        long endNs = System.nanoTime() + DURATION_MS * 1_000_000L;
        try (FileOutputStream fos = new FileOutputStream(outPath)) {
            while (System.nanoTime() < endNs) {
                int idx = codec.dequeueOutputBuffer(info, 100_000);
                if (idx >= 0) {
                    ByteBuffer buf = codec.getOutputBuffer(idx);
                    if (buf != null && info.size > 0) {
                        buf.position(info.offset);
                        buf.limit(info.offset + info.size);
                        byte[] data = new byte[info.size];
                        buf.get(data);
                        fos.write(data);
                        bytesWritten += info.size;
                        if ((info.flags & MediaCodec.BUFFER_FLAG_CODEC_CONFIG) == 0) {
                            frames++;
                        }
                    }
                    codec.releaseOutputBuffer(idx, false);
                }
            }
        }

        releaseDisplay(vd);
        codec.stop();
        codec.release();
        log("OK: " + bytesWritten + " bytes, ~" + frames + " frames en " + outPath);
        if (bytesWritten == 0) {
            log("ADVERTENCIA: 0 bytes — el encoder no produjo salida (posible R8).");
        }
    }

    private static int[] resolveDisplaySize() throws Exception {
        Class<?> dmgClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
        Object dmg = dmgClass.getMethod("getInstance").invoke(null);
        Object info = dmgClass.getMethod("getDisplayInfo", int.class).invoke(dmg, 0);
        Class<?> infoClass = info.getClass();
        int w = infoClass.getField("logicalWidth").getInt(info);
        int h = infoClass.getField("logicalHeight").getInt(info);
        return new int[]{w, h};
    }

    private static Object createMirrorDisplay(String name, int w, int h, Surface surface) throws Exception {
        Class<?> dmClass = Class.forName("android.hardware.display.DisplayManager");
        Method m = dmClass.getMethod("createVirtualDisplay",
                String.class, int.class, int.class, int.class, Surface.class);
        return m.invoke(null, name, w, h, 0, surface);
    }

    private static void releaseDisplay(Object virtualDisplay) {
        try {
            virtualDisplay.getClass().getMethod("release").invoke(virtualDisplay);
        } catch (Throwable ignored) {
            // best-effort
        }
    }

    private static void log(String msg) {
        System.out.println(TAG + " " + msg);
    }

    private Spike2Main() {
    }
}
