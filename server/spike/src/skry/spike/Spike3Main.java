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
    // Dimensiones por defecto de la pantalla virtual independiente (modo
    // new-display): 16:9 apaisado, cómodo para ver contenido en la PC y liviano
    // de decodificar.
    private static final int DEFAULT_ND_WIDTH = 1600;
    private static final int DEFAULT_ND_HEIGHT = 900;
    // Densidad de la pantalla virtual independiente (dpi). Afecta el escalado de
    // la UI de las apps lanzadas ahí; 320 es un mdpi/xhdpi razonable para teléfono.
    private static final int ND_DENSITY_DPI = 320;

    /** Opciones de arranque del server, parseadas de los args {@code clave=valor}. */
    private static final class Options {
        int maxDim = DEFAULT_MAX_DIMENSION;
        boolean newDisplay = false;
        int ndWidth = DEFAULT_ND_WIDTH;
        int ndHeight = DEFAULT_ND_HEIGHT;
        String app = null; // package a lanzar en el display independiente; null = home
    }

    public static void main(String[] args) {
        log("==== skry Spike 3 (server por socket) ====");
        Options opts = parseOptions(args);
        try {
            serve(opts);
        } catch (Throwable t) {
            log("FALLO: " + t);
            t.printStackTrace();
        }
    }

    /**
     * Parsea args {@code clave=valor} (orden libre). Compat: un arg suelto numérico
     * se interpreta como max-size (formato viejo del cliente).
     */
    private static Options parseOptions(String[] args) {
        Options o = new Options();
        for (String arg : args) {
            int eq = arg.indexOf('=');
            if (eq < 0) {
                // Compat con el formato viejo: un arg numérico suelto = max-size.
                // Cualquier otra cosa sin '=' es un arg desconocido (no asumir).
                if (arg.matches("-?\\d+")) {
                    applyOption(o, "max-size", arg);
                } else {
                    log("arg desconocido '" + arg + "' ignorado");
                }
                continue;
            }
            applyOption(o, arg.substring(0, eq), arg.substring(eq + 1));
        }
        return o;
    }

    private static void applyOption(Options o, String key, String value) {
        switch (key) {
            case "max-size":
                try {
                    int parsed = Integer.parseInt(value);
                    o.maxDim = parsed <= 0 ? Integer.MAX_VALUE : parsed; // 0 = sin límite
                } catch (NumberFormatException e) {
                    log("max-size invalido '" + value + "', uso " + DEFAULT_MAX_DIMENSION);
                }
                break;
            case "new-display":
                o.newDisplay = "1".equals(value) || "true".equalsIgnoreCase(value);
                break;
            case "nd-size":
                int x = value.indexOf('x');
                if (x > 0 && x < value.length() - 1) {
                    try {
                        int w = Integer.parseInt(value.substring(0, x));
                        int h = Integer.parseInt(value.substring(x + 1));
                        if (w > 0 && h > 0) {
                            o.ndWidth = w;
                            o.ndHeight = h;
                        } else {
                            log("nd-size con dimension <= 0 '" + value + "', uso "
                                    + DEFAULT_ND_WIDTH + "x" + DEFAULT_ND_HEIGHT);
                        }
                    } catch (NumberFormatException e) {
                        log("nd-size invalido '" + value + "', uso "
                                + DEFAULT_ND_WIDTH + "x" + DEFAULT_ND_HEIGHT);
                    }
                } else {
                    log("nd-size mal formado '" + value + "' (esperado ANCHOxALTO), uso "
                            + DEFAULT_ND_WIDTH + "x" + DEFAULT_ND_HEIGHT);
                }
                break;
            case "app":
                o.app = value.isEmpty() ? null : value;
                break;
            default:
                log("opcion desconocida '" + key + "' ignorada");
        }
    }

    private static void serve(Options opts) throws Exception {
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

            int width;
            int height;
            if (opts.newDisplay) {
                // Pantalla virtual INDEPENDIENTE: dimensiones elegidas por el cliente
                // (no se deriva del panel físico). Pares (el encoder lo requiere).
                width = opts.ndWidth & ~1;
                height = opts.ndHeight & ~1;
                log("Modo new-display: pantalla independiente " + width + "x" + height
                        + (opts.app != null ? " app=" + opts.app : " (home)"));
            } else {
                // Mirror: escalar el panel físico para aliviar el decode del cliente.
                int[] full = resolveDisplaySize();
                int[] size = scaleDown(full[0], full[1], opts.maxDim);
                width = size[0];
                height = size[1];
                log("Display fisico " + full[0] + "x" + full[1] + " -> captura " + width + "x" + height);
            }

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
            streamFrames(out, width, height, opts);
        }
        log("Sesion terminada.");
    }

    private static void streamFrames(DataOutputStream out, int width, int height, Options opts) throws Exception {
        MediaCodec codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_HEVC);
        MediaFormat format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_HEVC, width, height);
        format.setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface);
        format.setInteger(MediaFormat.KEY_BIT_RATE, BITRATE);
        format.setInteger(MediaFormat.KEY_FRAME_RATE, FRAME_RATE);
        format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1);
        codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE);
        Surface inputSurface = codec.createInputSurface();
        codec.start();

        // El display y el lanzamiento van DENTRO del try: si createIndependent o
        // el launch tiran excepción, el finally igual libera el codec (y el display
        // si llegó a crearse). Así nada queda colgado consumiendo batería.
        Object vd = null;
        MediaCodec.BufferInfo info = new MediaCodec.BufferInfo();
        long frames = 0;
        try {
            if (opts.newDisplay) {
                // Pantalla independiente + lanzar contenido EN ella (no en el panel físico).
                ShellContext.init();
                android.content.Context ctx = ShellContext.get();
                vd = VirtualDisplayFactory.createIndependent(
                        ctx, "skry-nd", width, height, ND_DENSITY_DPI, inputSurface);
                int displayId = VirtualDisplayFactory.getDisplayId(vd);
                if (opts.app != null) {
                    DisplayLauncher.launchApp(ctx, displayId, opts.app);
                } else {
                    DisplayLauncher.launchHome(ctx, displayId);
                }
            } else {
                vd = createMirrorDisplay("skry-spike3", width, height, inputSurface);
            }
            log("Streaming. Cerra el cliente para terminar.");

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

    // Lado mínimo creíble para un panel real; por debajo asumimos lectura stale.
    private static final int MIN_PLAUSIBLE_DIMENSION = 200;
    private static final int DISPLAY_INFO_RETRIES = 10;
    private static final long DISPLAY_INFO_RETRY_MS = 100;

    /**
     * Tamaño del panel. En la PRIMERA conexión tras un adb fresco, getDisplayInfo(0)
     * a veces devuelve un tamaño por defecto chico antes de que el DisplayInfo esté
     * poblado (de ahí el bug "primera corrida sale chica"). Reintentamos hasta que
     * el valor sea creíble, logueando cada lectura para diagnóstico.
     */
    private static int[] resolveDisplaySize() throws Exception {
        Class<?> dmgClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
        Object dmg = dmgClass.getMethod("getInstance").invoke(null);
        Method getDisplayInfo = dmgClass.getMethod("getDisplayInfo", int.class);

        int w = 0;
        int h = 0;
        for (int attempt = 1; attempt <= DISPLAY_INFO_RETRIES; attempt++) {
            Object info = getDisplayInfo.invoke(dmg, 0);
            if (info != null) {
                Class<?> infoClass = info.getClass();
                w = infoClass.getField("logicalWidth").getInt(info);
                h = infoClass.getField("logicalHeight").getInt(info);
            }
            log("getDisplayInfo intento " + attempt + ": " + w + "x" + h);
            if (w > 0 && h > 0 && Math.max(w, h) >= MIN_PLAUSIBLE_DIMENSION) {
                return new int[]{w, h};
            }
            Thread.sleep(DISPLAY_INFO_RETRY_MS);
        }
        // Agotados los reintentos: devolver lo último leído (mejor que fallar).
        log("getDisplayInfo no se estabilizo; uso " + w + "x" + h);
        return new int[]{Math.max(2, w), Math.max(2, h)};
    }

    private static Object createMirrorDisplay(String name, int w, int h, Surface surface) throws Exception {
        Class<?> dmClass = Class.forName("android.hardware.display.DisplayManager");
        Method m = dmClass.getMethod("createVirtualDisplay",
                String.class, int.class, int.class, int.class, Surface.class);
        return m.invoke(null, name, w, h, 0, surface);
    }

    private static void releaseDisplay(Object virtualDisplay) {
        if (virtualDisplay == null) {
            return;
        }
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
