package app.skry.capture

import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.hardware.display.VirtualDisplay
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.DisplayMetrics
import android.util.Log
import android.view.WindowManager
import androidx.core.app.NotificationCompat
import androidx.core.content.IntentCompat
import androidx.core.app.ServiceCompat
import app.skry.MainActivity
import app.skry.R
import app.skry.discovery.NsdAdvertiser
import app.skry.net.SkryProtocol
import java.io.BufferedOutputStream
import java.io.DataOutputStream
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Servicio foreground que captura la pantalla (MediaProjection), la codifica y la
 * sirve por TCP con el wire skry, anunciándose por mDNS. Sobrevive a que la app
 * pase a segundo plano: ese es el caso "dejá el celu y seguí transmitiendo".
 *
 * Atiende un cliente a la vez: acepta, hace el handshake, transmite hasta que el
 * cliente corta, y vuelve a aceptar.
 */
class CaptureService : Service() {

    // Campos tocados por el hilo main (onStartCommand, stopEverything) y el hilo
    // "skry-accept" (serverLoop/handleClient): @Volatile evita la data race.
    @Volatile
    private var running = false

    @Volatile
    private var projection: MediaProjection? = null

    @Volatile
    private var serverSocket: ServerSocket? = null

    @Volatile
    private var acceptThread: Thread? = null

    @Volatile
    private var nsd: NsdAdvertiser? = null

    // La pantalla virtual se crea UNA vez por proyección y se reusa entre clientes
    // (Android 14+ no permite más de una createVirtualDisplay por MediaProjection).
    // Las dimensiones de captura quedan fijas al crearla.
    @Volatile
    private var virtualDisplay: VirtualDisplay? = null

    @Volatile
    private var captureWidth = 0

    @Volatile
    private var captureHeight = 0

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopEverything()
            stopSelf()
            return START_NOT_STICKY
        }

        // Foreground PRIMERO: en Android 14+ hay que estar en foreground (tipo
        // mediaProjection) antes de obtener la MediaProjection.
        startForegroundNotification()

        val resultCode = intent?.getIntExtra(EXTRA_RESULT_CODE, Activity.RESULT_CANCELED)
            ?: Activity.RESULT_CANCELED
        val data = intent?.let { IntentCompat.getParcelableExtra(it, EXTRA_DATA, Intent::class.java) }
        if (resultCode != Activity.RESULT_OK || data == null) {
            Log.w(TAG, "sin permiso de captura válido; detengo")
            stopEverything()
            stopSelf()
            return START_NOT_STICKY
        }

        val mpm = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
        val mp = mpm.getMediaProjection(resultCode, data)
        if (mp == null) {
            Log.w(TAG, "getMediaProjection devolvió null; detengo")
            stopEverything()
            stopSelf()
            return START_NOT_STICKY
        }
        mp.registerCallback(object : MediaProjection.Callback() {
            override fun onStop() {
                Log.i(TAG, "MediaProjection detenida por el sistema/usuario")
                stopEverything()
                stopSelf()
            }
        }, Handler(Looper.getMainLooper()))
        projection = mp

        // Crear la pantalla virtual UNA sola vez (Android 14+ no permite más de una
        // por proyección; el segundo intento rompía la captura entera). Se reusa
        // entre clientes: cada uno engancha su propio codec con setSurface.
        val (w, h, dpi) = decideCapture()
        captureWidth = w
        captureHeight = h
        virtualDisplay = mp.createVirtualDisplay(
            "skry",
            w,
            h,
            dpi,
            // Sin VIRTUAL_DISPLAY_FLAG_PUBLIC: una pantalla virtual pública se
            // registra como pantalla secundaria del sistema (casi un monitor
            // externo), y en One UI eso bloqueaba el descarte de notificaciones de
            // otras apps mientras se transmitía. Privada captura igual; queda acotada
            // a skry. La surface se asigna por cliente (arranca en null).
            0,
            null,
            null,
            null,
        )
        if (virtualDisplay == null) {
            Log.e(TAG, "createVirtualDisplay devolvió null; detengo")
            stopEverything()
            stopSelf()
            return START_NOT_STICKY
        }

        running = true
        isRunning.set(true)
        startServer()
        return START_NOT_STICKY
    }

    private fun startServer() {
        val thread = Thread({ serverLoop() }, "skry-accept")
        acceptThread = thread
        thread.start()
    }

    private fun serverLoop() {
        try {
            // reuseAddress: si el servicio reinicia rápido, el puerto puede quedar
            // en TIME_WAIT; sin esto el bind falla.
            val server = ServerSocket()
            server.reuseAddress = true
            server.bind(java.net.InetSocketAddress(PORT))
            serverSocket = server
            nsd = NsdAdvertiser(this).also { it.register(server.localPort, serviceName()) }
            Log.i(TAG, "escuchando en :${server.localPort}")
            while (running) {
                val socket = try {
                    server.accept()
                } catch (e: Exception) {
                    if (running) Log.w(TAG, "accept falló: $e")
                    break
                }
                handleClient(socket)
            }
        } catch (e: Exception) {
            Log.e(TAG, "servidor terminó con error: $e")
        } finally {
            nsd?.unregister()
            runCatching { serverSocket?.close() }
        }
    }

    private fun handleClient(socket: Socket) {
        val vd = virtualDisplay ?: return
        socket.use { s ->
            runCatching { s.tcpNoDelay = true }
            val input = s.getInputStream()
            val streamType = input.read()
            if (streamType != SkryProtocol.STREAM_VIDEO) {
                Log.w(TAG, "tipo de stream inesperado: $streamType")
                return
            }
            val width = captureWidth
            val height = captureHeight
            val out = DataOutputStream(BufferedOutputStream(s.getOutputStream()))
            SkryProtocol.writeHandshake(out, SkryProtocol.CODEC_H265, width, height, Build.MODEL)
            Log.i(TAG, "cliente conectado; captura ${width}x$height")

            // Detección de cliente muerto: con la pantalla quieta el encoder no emite
            // frames, así que un fallo de escritura nunca llega y un cliente caído
            // bloquearía el servidor para siempre (handleClient no retornaría y no se
            // aceptarían nuevos clientes). Este hilo lee del socket —el cliente no
            // manda nada tras el byte de tipo— para que read() devuelva -1 o tire al
            // desconectarse, y cortamos al instante aunque no haya frames en vuelo.
            val clientAlive = AtomicBoolean(true)
            val liveness = Thread({
                try {
                    while (running && input.read() >= 0) { /* el cliente no envía datos */ }
                } catch (_: Exception) {
                    // socket cerrado o caído: se refleja en clientAlive abajo
                } finally {
                    clientAlive.set(false)
                }
            }, "skry-liveness").apply { isDaemon = true; start() }

            val encoder = ScreenEncoder(vd, width, height)
            try {
                encoder.start()
                encoder.drain(
                    onFrame = { pts, frameFlags, payload, len ->
                        SkryProtocol.writeFrame(out, pts, frameFlags, payload, len)
                    },
                    shouldStop = { !running || s.isClosed || !clientAlive.get() },
                )
            } catch (e: Exception) {
                Log.i(TAG, "cliente desconectado: ${e.message}")
            } finally {
                encoder.release()
                // Desbloquear el read() del liveness y esperarlo brevemente.
                runCatching { s.shutdownInput() }
                liveness.join(LIVENESS_JOIN_MS)
                if (liveness.isAlive) {
                    Log.w(TAG, "el hilo liveness no terminó en ${LIVENESS_JOIN_MS}ms")
                }
            }
        }
    }

    /** Tamaño de captura: panel real escalado a [MAX_DIMENSION], dimensiones pares. */
    private fun decideCapture(): Triple<Int, Int, Int> {
        val wm = getSystemService(Context.WINDOW_SERVICE) as WindowManager
        val dpi = resources.configuration.densityDpi
        val (fullW, fullH) = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val bounds = wm.currentWindowMetrics.bounds
            Pair(bounds.width(), bounds.height())
        } else {
            val metrics = DisplayMetrics()
            @Suppress("DEPRECATION")
            wm.defaultDisplay.getRealMetrics(metrics)
            Pair(metrics.widthPixels, metrics.heightPixels)
        }
        val (w, h) = Scaling.scaleDown(fullW, fullH, MAX_DIMENSION)
        return Triple(w, h, dpi)
    }

    private fun startForegroundNotification() {
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Captura de pantalla",
                NotificationManager.IMPORTANCE_LOW,
            )
            nm.createNotificationChannel(channel)
        }

        val openIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val stopIntent = PendingIntent.getService(
            this,
            1,
            Intent(this, CaptureService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val notification: Notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("skry está transmitiendo")
            .setContentText("Tu pantalla se está enviando a la PC")
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setOngoing(true)
            .setContentIntent(openIntent)
            .addAction(0, "Detener", stopIntent)
            .build()

        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION
        } else {
            0
        }
        ServiceCompat.startForeground(this, NOTIF_ID, notification, type)
    }

    private fun serviceName(): String = "skry ${Build.MODEL}"

    private fun stopEverything() {
        running = false
        isRunning.set(false)
        nsd?.unregister()
        nsd = null
        runCatching { serverSocket?.close() }
        serverSocket = null
        // Esperar a que el hilo de captura termine (sale en <=100ms al ver running=false)
        // antes de soltar la projection, para no usarla ya detenida. Cota corta: no ANR.
        val thread = acceptThread
        acceptThread = null
        thread?.let { runCatching { it.join(JOIN_TIMEOUT_MS) } }
        val stillRunning = thread?.isAlive == true
        if (stillRunning) {
            Log.w(TAG, "el hilo de captura no terminó en ${JOIN_TIMEOUT_MS}ms")
        }
        // Liberar la pantalla virtual SOLO si el encoder ya soltó su surface (hilo
        // terminado). Si el hilo sigue vivo, liberarla acá sería un use-after-release
        // nativo; projection.stop() la invalida igual abajo.
        if (!stillRunning) {
            runCatching { virtualDisplay?.release() }
        }
        virtualDisplay = null
        runCatching { projection?.stop() }
        projection = null
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
    }

    override fun onDestroy() {
        stopEverything()
        super.onDestroy()
    }

    companion object {
        private const val TAG = "skry-capture"
        private const val CHANNEL_ID = "skry_capture"
        private const val NOTIF_ID = 1
        /** Puerto TCP del servidor skry (lo usa el cliente con --connect IP:PUERTO). */
        const val PORT = 7345
        // Captura a la resolución nativa del panel, igual que el spike validado en
        // device (max-size 0). 4096 es un techo de seguridad para el decode del
        // cliente, no un downscale real: ningún teléfono lo supera, así que en la
        // práctica captura nativo. Bajarlo reintroduce el downscale, que cambia el
        // aspecto respecto al nativo y mete barras de letterbox en los bordes.
        private const val MAX_DIMENSION = 4096
        private const val JOIN_TIMEOUT_MS = 1500L
        private const val LIVENESS_JOIN_MS = 500L

        const val ACTION_STOP = "app.skry.action.STOP"
        const val EXTRA_RESULT_CODE = "result_code"
        const val EXTRA_DATA = "result_data"

        /**
         * Si el servicio está transmitiendo. Permite que la UI resincronice su
         * estado al volver (el sistema puede matar el servicio sin matar el
         * proceso, dejando la UI mostrando "transmitiendo" de mentira).
         *
         * Vive en el proceso: si Android mata el proceso entero (OOM/forceStop)
         * arranca de nuevo en false al relanzar — correcto. Dentro de un proceso
         * vivo, onDestroy -> stopEverything es la única vía de terminación.
         */
        val isRunning = AtomicBoolean(false)
    }
}
