rootProject.name = "skry-server"

// El modulo :protocol es Kotlin/JVM puro (sin Android): implementa el wire de
// skry y se testea sin el SDK. El modulo :app (Android, captura+encode) se
// agrega cuando se aborde la captura; necesita el Android SDK.
include(":protocol")
