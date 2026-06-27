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

        running = true
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
        val mp = projection ?: return
        socket.use { s ->
            runCatching { s.tcpNoDelay = true }
            val streamType = s.getInputStream().read()
            if (streamType != SkryProtocol.STREAM_VIDEO) {
                Log.w(TAG, "tipo de stream inesperado: $streamType")
                return
            }
            val (width, height, dpi) = decideCapture()
            val out = DataOutputStream(BufferedOutputStream(s.getOutputStream()))
            SkryProtocol.writeHandshake(out, SkryProtocol.CODEC_H265, width, height, Build.MODEL)
            Log.i(TAG, "cliente conectado; captura ${width}x$height")

            val encoder = ScreenEncoder(mp, width, height, dpi)
            try {
                encoder.start()
                encoder.drain(
                    onFrame = { pts, frameFlags, payload ->
                        SkryProtocol.writeFrame(out, pts, frameFlags, payload)
                    },
                    shouldStop = { !running || s.isClosed },
                )
            } catch (e: Exception) {
                Log.i(TAG, "cliente desconectado: ${e.message}")
            } finally {
                encoder.release()
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
        val (w, h) = scaleDown(fullW, fullH, MAX_DIMENSION)
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
        nsd?.unregister()
        nsd = null
        runCatching { serverSocket?.close() }
        serverSocket = null
        // Esperar a que el hilo de captura termine (sale en <=100ms al ver running=false)
        // antes de soltar la projection, para no usarla ya detenida. Cota corta: no ANR.
        acceptThread?.let { t -> runCatching { t.join(JOIN_TIMEOUT_MS) } }
        acceptThread = null
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
        private const val MAX_DIMENSION = 2400
        private const val JOIN_TIMEOUT_MS = 1500L

        const val ACTION_STOP = "app.skry.action.STOP"
        const val EXTRA_RESULT_CODE = "result_code"
        const val EXTRA_DATA = "result_data"

        /** Escala (w,h) para que el lado mayor no supere [maxDim], con dims pares. */
        fun scaleDown(w: Int, h: Int, maxDim: Int): Pair<Int, Int> {
            val longest = maxOf(w, h)
            if (longest <= maxDim) return evenPair(w, h)
            val f = maxDim.toDouble() / longest
            return evenPair((w * f).toInt(), (h * f).toInt())
        }

        private fun evenPair(w: Int, h: Int): Pair<Int, Int> =
            Pair(maxOf(2, w and 1.inv()), maxOf(2, h and 1.inv()))
    }
}
