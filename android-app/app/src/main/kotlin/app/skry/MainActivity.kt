package app.skry

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.projection.MediaProjectionManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import app.skry.capture.CaptureService
import app.skry.net.LocalAddress
import app.skry.update.UpdateChecker
import app.skry.update.UpdateInfo
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Cast
import androidx.compose.material.icons.filled.CastConnected
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material3.Button
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import app.skry.ui.theme.SkryTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            SkryTheme {
                SkryRoot()
            }
        }
    }
}

/**
 * Raíz que conecta la UI con el sistema: pide el permiso de captura
 * (MediaProjection) y arranca/detiene el [CaptureService]. En Android 13+ pide
 * antes el permiso de notificaciones (la captura corre en foreground service).
 */
@Composable
private fun SkryRoot() {
    val context = LocalContext.current
    var capturing by rememberSaveable { mutableStateOf(false) }
    var update by remember { mutableStateOf<UpdateInfo?>(null) }

    // Chequeo de actualización una vez al abrir: best-effort, sólo avisa.
    LaunchedEffect(Unit) {
        UpdateChecker.checkAsync(BuildConfig.VERSION_NAME) { update = it }
    }

    val projectionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { result ->
        val data = result.data
        if (result.resultCode == Activity.RESULT_OK && data != null) {
            val svc = Intent(context, CaptureService::class.java).apply {
                putExtra(CaptureService.EXTRA_RESULT_CODE, result.resultCode)
                putExtra(CaptureService.EXTRA_DATA, data)
            }
            ContextCompat.startForegroundService(context, svc)
            capturing = true
        }
    }

    val notifLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) {
        // Con o sin permiso de notificaciones seguimos: la notificación es
        // deseable pero no bloquea la captura.
        requestProjection(context, projectionLauncher)
    }

    // Dirección a la que conectar la PC mientras transmite (IP local : puerto).
    val serverAddress = remember(capturing) {
        if (capturing) LocalAddress.wifiIpv4()?.let { "$it:${CaptureService.PORT}" } else null
    }

    SkryApp(
        capturing = capturing,
        serverAddress = serverAddress,
        update = update,
        onOpenUpdate = {
            // runCatching: algunos dispositivos no tienen browser -> ACTION_VIEW
            // tiraría ActivityNotFoundException y crashearía la app.
            update?.let {
                runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(it.url))) }
            }
        },
        onToggle = {
            if (capturing) {
                context.startService(
                    Intent(context, CaptureService::class.java).setAction(CaptureService.ACTION_STOP),
                )
                capturing = false
            } else if (needsNotificationPermission(context)) {
                notifLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
            } else {
                requestProjection(context, projectionLauncher)
            }
        },
    )
}

private fun requestProjection(
    context: Context,
    launcher: ActivityResultLauncher<Intent>,
) {
    val mpm = context.getSystemService(MediaProjectionManager::class.java)
    launcher.launch(mpm.createScreenCaptureIntent())
}

private fun needsNotificationPermission(context: Context): Boolean {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return false
    return ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
        PackageManager.PERMISSION_GRANTED
}

/** Modos de captura. El espejo replica el panel; "aparte" usa una pantalla virtual. */
private val MODES = listOf("Espejo", "Pantalla aparte")

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SkryApp(
    capturing: Boolean,
    serverAddress: String?,
    update: UpdateInfo?,
    onOpenUpdate: () -> Unit,
    onToggle: () -> Unit,
) {
    var mode by remember { mutableIntStateOf(0) }

    Scaffold(
        // "skry" en minúscula: es el nombre de marca, intencional.
        topBar = { CenterAlignedTopAppBar(title = { Text("skry") }) },
    ) { inner ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(inner)
                .padding(horizontal = 20.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            if (update != null) {
                UpdateBanner(version = update.version, onView = onOpenUpdate)
            }

            StatusCard(capturing = capturing, serverAddress = serverAddress)

            Text(
                text = "Modo de captura",
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
                MODES.forEachIndexed { index, label ->
                    SegmentedButton(
                        selected = mode == index,
                        onClick = { mode = index },
                        shape = SegmentedButtonDefaults.itemShape(index, MODES.size),
                    ) {
                        Text(label)
                    }
                }
            }

            Spacer(Modifier.weight(1f))

            CaptureButton(capturing = capturing, onToggle = onToggle)
        }
    }
}

@Composable
private fun StatusCard(capturing: Boolean, serverAddress: String?) {
    val title = if (capturing) "Transmitiendo" else "Listo para transmitir"
    val subtitle = when {
        capturing && serverAddress != null -> "Conectá la PC a  $serverAddress"
        capturing -> "La PC está recibiendo tu pantalla"
        else -> "Tocá «Iniciar» y aceptá el permiso de captura"
    }
    val icon = if (capturing) Icons.Filled.CastConnected else Icons.Filled.Cast

    ElevatedCard(
        modifier = Modifier
            .fillMaxWidth()
            .semantics { contentDescription = "$title. $subtitle" },
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(20.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            StatusIcon(icon)
            Spacer(Modifier.width(16.dp))
            Column {
                Text(text = title, style = MaterialTheme.typography.titleMedium)
                Text(
                    text = subtitle,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun StatusIcon(icon: ImageVector) {
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.primaryContainer,
        modifier = Modifier.size(48.dp),
    ) {
        Row(
            horizontalArrangement = Arrangement.Center,
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.fillMaxSize(),
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onPrimaryContainer,
            )
        }
    }
}

@Composable
private fun CaptureButton(capturing: Boolean, onToggle: () -> Unit) {
    Button(
        onClick = onToggle,
        modifier = Modifier
            .fillMaxWidth()
            .height(56.dp),
    ) {
        Icon(
            imageVector = if (capturing) Icons.Filled.Stop else Icons.Filled.PlayArrow,
            contentDescription = if (capturing) "Detener captura" else "Iniciar captura",
        )
        Spacer(Modifier.width(8.dp))
        Text(if (capturing) "Detener" else "Iniciar captura")
    }
}

@Composable
private fun UpdateBanner(version: String, onView: () -> Unit) {
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.tertiaryContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = Icons.Filled.SystemUpdate,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onTertiaryContainer,
            )
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "Actualización disponible ($version)",
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onTertiaryContainer,
                )
                Text(
                    text = "Descargala cuando quieras desde las releases",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onTertiaryContainer,
                )
            }
            Spacer(Modifier.width(12.dp))
            FilledTonalButton(onClick = onView) { Text("Ver") }
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun SkryAppPreview() {
    SkryTheme {
        SkryApp(
            capturing = true,
            serverAddress = "192.168.1.50:7345",
            update = UpdateInfo("0.2.0", "https://example.com"),
            onOpenUpdate = {},
            onToggle = {},
        )
    }
}
