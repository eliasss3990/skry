# ADR 0003: Defaults conservadores de fluidez

- Estado: Aceptada
- Fecha: 2026-06-16

## Contexto

El plan fijaba como default **144 FPS / H.265 / hardware decode**. Codificar en
tiempo real 144 FPS de pantalla completa en H.265 supera la capacidad del
encoder de muchos teléfonos y de la mayoría de las redes; aun en gama alta, el
sostén depende de térmica y resolución. Un default agresivo arriesga una primera
experiencia mala (tearing, drops, latencia) en cualquier dispositivo que no sea
el tope de gama en condiciones ideales.

## Decisión

El default de arranque es **conservador y dependiente del dispositivo**:

- **Framerate**: la tasa nativa del panel, con tope por defecto en **60 FPS**.
- **Códec**: se elige según lo que el server reporte en el handshake. Se prefiere
  H.265 si hay encoder por hardware disponible y el cliente puede decodificarlo;
  si no, H.264.
- **Decode**: por hardware si está disponible, con fallback transparente a CPU
  (ver resiliencia).

Las marchas altas (**120 / 144 FPS**) son **opt-in explícito** por flag
(`--gear 144`). El sistema de marchas sólo *sube* si el dispositivo y la red lo
sostienen; ante inestabilidad baja solo.

### Relación entre "tasa nativa del panel" y las marchas discretas

Las marchas son tres valores fijos (60/120/144). La "tasa nativa del panel" se
usa sólo como **tope**: el FPS efectivo objetivo es `min(tasa_nativa, fps_marcha)`.
Un panel de 90 Hz con marcha `Low` (60) captura a 60; con marcha `Mid` (120)
captura a 90 (acotado por el panel), no a 120. Es decir, la marcha fija un
*techo* de FPS y el panel otro; gana el menor. `Gear::from_fps` mapea un FPS
pedido por el usuario a la marcha mínima que lo cubre (techo): pedir 70 da `Mid`.

## Consecuencias

- **Positivas**: buena experiencia desde el primer arranque en cualquier
  teléfono; el ajuste por potencia es real (el handshake informa capacidades y
  el cliente elige un default sensato), y el usuario avanzado conserva control
  total por flags.
- **Negativas**: quien quiera 144 FPS debe pedirlo explícitamente. Es aceptable:
  es una decisión informada, no un default que decepciona.
