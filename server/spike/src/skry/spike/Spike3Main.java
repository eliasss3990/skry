package skry.spike;

import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaFormat;
import android.net.LocalServerSocket;
import android.net.LocalSocket;
import android.os.Build;
import android.view.Surface;

import java.io.DataOutputStream;
import java.io.InputStream;
import java.lang.reflect.Method;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;

/**
 * Spike 3 (lado server): transmite el stream H.265 sobre el túnel ADB usando el
 * wire de skry, para que el cliente Rust (skry-adb + skry-proto) lo reciba.
 *
 * Escucha en el localabstract socket "skry". El cliente conecta, manda 1 byte de
 * tipo de stream (0x00 = video), y el server responde con el handshake y luego el
 * stream de frames (cabecera pts/flags/len + payload NAL). Reusa la captura y el
 * encoder validados en los spikes 1 y 2. Codificación big-endian (DataOutputStream),
 * espejo de skry-proto.
 */
public final class Spike3Main {

    private static final String TAG = "[skry-spike3]";
    private static final String SOCKET_NAME = "skry";
    private static final byte[] MAGIC = {0x53, 0x4B, 0x52, 0x59}; // "SKRY"
    private static final int PROTOCOL_VERSION = 1;
    private static final int CODEC_H265 = 1;
    private static final int STREAM_VIDEO = 0x00;
    private static final int BITRATE = 40_000_000;
    private static final int FRAME_RATE = 60;
    // Cap por defecto del lado más largo de la captura (si el cliente no manda
    // uno). Capturar más resolución de la que el monitor del cliente puede mostrar
    // sólo agrega trabajo (decode + transferencias) sin calidad visible. 2400 es
    // el punto dulce medido: calidad casi full y ~100fps fluidos.
    private static final int DEFAULT_MAX_DIMENSION = 2400;

    public static void main(String[] args) {
        log("==== skry Spike 3 (server por socket) ====");
        int maxDim = DEFAULT_MAX_DIMENSION;
        if (args.length > 0) {
            try {
                int parsed = Integer.parseInt(args[0]);
                // 0 (o negativo) = sin límite -> panel completo.
                maxDim = parsed <= 0 ? Integer.MAX_VALUE : parsed;
            } catch (NumberFormatException e) {
                log("max-size invalido '" + args[0] + "', uso " + DEFAULT_MAX_DIMENSION);
            }
        }
        try {
            serve(maxDim);
        } catch (Throwable t) {
            log("FALLO: " + t);
            t.printStackTrace();
        }
    }

    private static void serve(int maxDim) throws Exception {
        log("Escuchando en localabstract:" + SOCKET_NAME + " ...");
        try (LocalServerSocket server = new LocalServerSocket(SOCKET_NAME);
             LocalSocket client = server.accept()) {
            log("Cliente conectado.");

            // 1) Leer el byte de tipo de stream (esperado: video).
            InputStream in = client.getInputStream();
            int streamType = in.read();
            if (streamType != STREAM_VIDEO) {
                log("tipo de stream inesperado: " + streamType + " (se esperaba video)");
                return;
            }

            // Escalar la captura para aliviar el decode (software) del cliente:
            // un mirror a resolución reducida se decodifica mucho más rápido y,
            // a igual bitrate, se ve mejor. El virtual display espeja la pantalla
            // completa downscaleada a estas dimensiones.
            int[] full = resolveDisplaySize();
            int[] size = scaleDown(full[0], full[1], maxDim);
            int width = size[0];
            int height = size[1];
            log("Display fisico " + full[0] + "x" + full[1] + " -> captura " + width + "x" + height);

            DataOutputStream out = new DataOutputStream(client.getOutputStream());
            // 2) Handshake: magic + version + codec + w + h + deviceName.
            out.write(MAGIC);
            out.writeShort(PROTOCOL_VERSION);
            out.writeByte(CODEC_H265);
            out.writeShort(width);
            out.writeShort(height);
            writeString(out, Build.MODEL);
            out.flush();
            log("Handshake enviado (" + width + "x" + height + ", " + Build.MODEL + ").");

            // 3) Encoder + virtual display y bombeo de frames al socket.
            streamFrames(out, width, height);
        }
        log("Sesion terminada.");
    }

    private static void streamFrames(DataOutputStream out, int width, int height) throws Exception {
        MediaCodec codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_HEVC);
        MediaFormat format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_HEVC, width, height);
        format.setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface);
        format.setInteger(MediaFormat.KEY_BIT_RATE, BITRATE);
        format.setInteger(MediaFormat.KEY_FRAME_RATE, FRAME_RATE);
        format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1);
        codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE);
        Surface inputSurface = codec.createInputSurface();
        codec.start();

        Object vd = createMirrorDisplay("skry-spike3", width, height, inputSurface);
        log("Streaming. Cerra el cliente para terminar.");

        MediaCodec.BufferInfo info = new MediaCodec.BufferInfo();
        long frames = 0;
        try {
            // Hasta que el cliente cierre (la escritura al socket tira IOException).
            while (true) {
                int idx = codec.dequeueOutputBuffer(info, 100_000);
                if (idx < 0) {
                    continue;
                }
                ByteBuffer buf = codec.getOutputBuffer(idx);
                if (buf != null && info.size > 0) {
                    buf.position(info.offset);
                    buf.limit(info.offset + info.size);
                    byte[] payload = new byte[info.size];
                    buf.get(payload);

                    int flags = 0;
                    if ((info.flags & MediaCodec.BUFFER_FLAG_KEY_FRAME) != 0) flags |= 0x01;
                    if ((info.flags & MediaCodec.BUFFER_FLAG_CODEC_CONFIG) != 0) flags |= 0x02;

                    // Cabecera de frame: pts(u64) + flags(u8) + len(u32) + payload.
                    out.writeLong(info.presentationTimeUs);
                    out.writeByte(flags);
                    out.writeInt(payload.length);
                    out.write(payload);
                    out.flush();
                    if ((flags & 0x02) == 0) frames++;
                }
                codec.releaseOutputBuffer(idx, false);
            }
        } catch (java.io.IOException e) {
            log("Cliente desconectado (" + e.getMessage() + "). Frames enviados: " + frames);
        } finally {
            releaseDisplay(vd);
            codec.stop();
            codec.release();
        }
        log("Total frames enviados: " + frames);
    }

    private static void writeString(DataOutputStream out, String s) throws Exception {
        byte[] bytes = s.getBytes(StandardCharsets.UTF_8);
        out.writeShort(bytes.length);
        out.write(bytes);
    }

    /** Escala (w,h) para que el lado más largo no supere maxDim, con dims pares. */
    private static int[] scaleDown(int w, int h, int maxDim) {
        int longest = Math.max(w, h);
        if (longest <= maxDim) {
            return new int[]{w, h};
        }
        double f = (double) maxDim / longest;
        int nw = ((int) Math.round(w * f)) & ~1; // par (el encoder lo requiere)
        int nh = ((int) Math.round(h * f)) & ~1;
        return new int[]{Math.max(2, nw), Math.max(2, nh)};
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

    private Spike3Main() {
    }
}
