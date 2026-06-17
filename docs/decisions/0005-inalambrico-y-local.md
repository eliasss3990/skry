# ADR 0005: Operación inalámbrica y 100% local (sin internet ni datos móviles)

- Estado: Aceptada
- Fecha: 2026-06-16

## Contexto

Dos requisitos explícitos del usuario, aclarados durante el desarrollo:

1. **Inalámbrico, a unos metros, sin cable.** Poder espejar con el teléfono a
   unos metros de la notebook, sin estar atado por USB.
2. **Sin depender de saldo / datos móviles / internet.** El motivo de fondo de
   construir software propio en vez de usar soluciones de streaming existentes:
   no pasar el video por la nube ni consumir un plan de datos.

## Decisión

`skry` transmite **exclusivamente por el enlace local**, nunca por internet:

- **Transporte inalámbrico = ADB sobre Wi-Fi** (depuración inalámbrica de
  Android). El túnel ADB de ADR-0001 es agnóstico al medio: funciona igual sobre
  USB o sobre Wi-Fi. En Android 11+ (el S24 Ultra incluido) el emparejamiento se
  hace por código, **sin necesidad de conectar el cable ni una vez**.
- **Cero nube, cero internet, cero datos móviles.** El video viaja sólo entre el
  teléfono y la notebook sobre la red local. Aunque el router no tenga internet,
  el tráfico LAN es local y gratuito. No hay servidores intermedios ni costo de
  ancho de banda externo.

USB queda como camino **opcional** (menor latencia, o para el primer
emparejamiento en dispositivos viejos), no como requisito.

### Alcance de redes

- **Con red Wi-Fi local compartida** (router/AP, con o sin internet): cubierto
  por ADB sobre Wi-Fi. Es el caso del MVP.
- **Sin ninguna red compartida** (no hay router/AP): requiere **Wi-Fi Direct**
  (enlace directo teléfono↔notebook, sin router). Esto eleva la prioridad de
  Wi-Fi Direct como la vía a la independencia total, pero sigue siendo
  **posterior al MVP** (ver ADR-0001 para los problemas de implementarlo desde
  un `app_process` sin contexto de app). La abstracción `skry-transport` lo
  admite sin reescritura.

## Consecuencias

- **Positivas**: cumple los dos requisitos sin costo recurrente ni dependencia
  externa; el MVP ya es inalámbrico vía ADB sobre Wi-Fi; el diseño no se cierra
  al caso sin-router (Wi-Fi Direct entra como capa de transporte futura).
- **Negativas**: el MVP necesita una red Wi-Fi local común entre ambos
  dispositivos. Mitigación: documentarlo claramente; Wi-Fi Direct cubre el caso
  sin-router cuando se priorice.
- **Operativa**: el cliente debe soportar descubrir/conectar dispositivos por
  red (`adb connect <ip>:<puerto>`, emparejamiento inalámbrico), no sólo por
  USB. Esto se refleja en los requisitos del módulo `skry-adb`.
