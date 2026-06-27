package app.skry.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

private val DarkColors = darkColorScheme(
    primary = SkryBlue,
    secondary = SkryBlueDark,
    background = SkryInk,
    surface = SkrySurfaceDark,
)

private val LightColors = lightColorScheme(
    primary = SkryBlueDark,
    secondary = SkryBlue,
    surface = SkrySurfaceLight,
    background = SkrySurfaceLight,
)

/**
 * Tema de la app. Usa dynamic color (Material You) en Android 12+ para integrarse
 * con el wallpaper del usuario; cae a la paleta de marca en versiones previas.
 */
@Composable
fun SkryTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = true,
    content: @Composable () -> Unit,
) {
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> DarkColors
        else -> LightColors
    }
    MaterialTheme(
        colorScheme = colorScheme,
        typography = Typography,
        content = content,
    )
}
