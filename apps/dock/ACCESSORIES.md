# Batería de accesorios en la barra superior

La opción **Ajustes → Barra superior → Mostrar batería de accesorios** está
desactivada por defecto. Al activarla, la detección es automática mientras exista
la barra superior. Un ratón y unos auriculares conectados muestran su icono y
porcentaje junto al idioma y la batería del equipo. No hay popup ni botón Bluetooth vacío.
Sin accesorios conectados no se reserva espacio. Los indicadores son pasivos y
conservan el cursor de flecha, la tipografía, colores y escala de la barra.

## Datos y compatibilidad

Bluetooth ya emparejado y adaptadores USB incluidos en la lista de esta entrega,
siempre con confirmación de conexión del accesorio, no solo del receptor. Se elige un dispositivo por categoría en orden estable, con prioridad
para la salida actual de audio. La batería viene de metadatos de Windows unidos
por Container ID; la propiedad de compatibilidad no es un contrato universal.

Sin nivel único se muestra «—»; 0 % sigue siendo un valor válido. Niveles
contradictorios no se promedian y nombres iguales no fusionan dispositivos.
Un fallo o lectura incompleta oculta los indicadores hasta recuperar datos;
no se presentan porcentajes anteriores como actuales.

Para USB solo se abre la colección de fabricante del VID/PID autorizado, después
de comprobar identidad, usage y longitudes de informe. No hay conexiones GATT,
búsqueda de vecinos ni consultas a receptores desconocidos. No se conoce la carga del estuche, valores por
auricular o el instante de medición física. `accessoryBattery` guarda la opción;
si falta en un archivo existente, vale `false`. La selección antigua
`accessoryPins` se ignora: los dispositivos se eligen automáticamente.

Todavía no hay hardware inalámbrico para contrastar niveles y cambios reales.
No se anuncian marcas o modelos compatibles. Windows 10 requiere prueba propia.

## Recursos y recuperación

Con la opción apagada no se crea el monitor, su proceso auxiliar, suscripciones
ni consultas USB. Apagarla libera el monitor existente y retira los indicadores.
Reanudación, cambios de audio y reintentos no pueden reactivarlo. Solo queda la
preferencia en memoria; las mediciones del lector de abajo corresponden a la
opción activada, no al estado inicial de la app.

El hilo gráfico solo compara dos lecturas pequeñas: cambios de nombre o de
dispositivos no representados no reconstruyen la barra. El mismo ejecutable con
`--accessories-worker` aloja los proveedores WinRT en MTA, fuera del hilo gráfico.
Tres watchers filtrados, cola no bloqueante de 256 eventos y agrupación de ráfagas
durante 120 ms evitan barridos por fotograma. Bluetooth sigue usando eventos;
los receptores USB tienen la política de consulta descrita más abajo.

El padre limita cada operación a cinco segundos y cada mensaje a 64 KiB/32
dispositivos. Quitar la barra termina el auxiliar sin `join` en la UI. Un Job
Object con `KILL_ON_JOB_CLOSE` también lo termina si el dock falla. El lector
revoca su espera antes de liberar el contexto. Reanudación y cambio de salida
de audio reconstruyen las suscripciones; tras errores, el reloj ya existente
permite reintentar como máximo una vez por minuto, sin temporizador nuevo.

## Adaptadores USB iniciales · 11c

Son implementaciones en validación, todavía sin compatibilidad física certificada.

| Receptor/familia | VID:PID | Colección y lectura |
|---|---|---|
| Logitech Unifying | 046D:C52B, 046D:C532 | FF00:0002, informes HID++ largos de 20 bytes. Slots 1–6; ping con eco y consulta del tipo. Porcentaje solo si 1004 anuncia estado de carga real. |
| Logitech Bolt | 046D:C548 | Misma política. Un dispositivo HID++ 1.0 o sin batería exacta no recibe un porcentaje inventado. |
| SteelSeries Nova 7 original | 1038:2202 | Interfaz 3, FFC0:0001; informe B0 con estado del enlace. Sus niveles discretos permanecen «—», no 25/50/75 % ficticios. |
| SteelSeries Nova 7 actualizado / Gen 2 | 1038:22A1, 1038:227E | Misma colección, porcentaje directo 0–100 y estado de conexión. |

No se amplía por marca o similitud del nombre. El filtro de Windows limita la
enumeración a estas colecciones; antes de escribir una solicitud de lectura se
verifican de nuevo VID/PID, descriptor y tamaños con un handle sin acceso a datos.
Solo después se abre lectura/escritura compartida de esa colección. Los paquetes
son consultas de estado, nunca cambios de ajustes, luces, emparejamiento o firmware.
No se usan curvas de voltaje ni se fuerza un valor a entrar en 0–100.

### Política de consumo y aislamiento

- Sin interfaces autorizadas: ningún proceso de consulta USB ni hilo del pool.
- Hasta ocho interfaces, una consulta en curso, cola de un trabajo y generaciones
  para descartar respuestas de un receptor retirado/reconectado. Quitar el último
  receptor libera el pool al acabar su trabajo acotado.
- Una consulta inicial al detectar la interfaz y, como respaldo, cada 60 segundos
  mientras la barra esté visible. El receptor puede responder que el accesorio está
  apagado: también se limita a 60 segundos ese intento de descubrirlo.
- Pausa mientras la barra esté auto-oculta o cubierta. Dos eventos de kernel comunican
  visibilidad; no hay un timer de UI ni escrituras bloqueantes a una tubería.
- Cada consulta corre en otro `muelle.exe --accessory-usb-query`, con Job Object y
  plazo de dos segundos. HID usa I/O overlapped, presupuestos de 150 ms por intercambio
  y 1.250 ms para la secuencia. Si un controlador no cancela, solo se termina ese hijo;
  el coordinador Bluetooth no espera la consulta.
- Ante errores: se retira su lectura y la espera crece desde dos hasta quince minutos.
  Los cambios de metadatos no reinician el plazo. Reanudación o cambio de salida de
  audio reconstruyen el lector; no se registra la identidad del periférico en logs.

El intervalo de 60 segundos sustituye al inicial de cinco minutos. En operación
normal, por interfaz visible, pasa de unas 12 a unas 60 consultas por hora
(aproximadamente 48 adicionales). El coste de CPU acumulado de las consultas
crece con su frecuencia; no implica que el consumo total de Muelle se multiplique
por cinco. Se mantienen el mismo pool, una consulta simultánea y el mismo límite
de memoria de cada proceso temporal.

Sin receptor identificado o con la barra oculta no hay consultas USB adicionales.
El coste real con un receptor sigue pendiente de medición: incluye creación del
proceso, trabajo de sus controladores y posible actividad del periférico. El
timeout de dos segundos es tiempo de pared; no equivale a dos segundos de CPU.

**Límite funcional:** desenchufar el USB se detecta por eventos. Encender/apagar el
accesorio dejando el receptor puesto puede reflejarse hasta la siguiente consulta
(60 segundos visibles; después de errores, hasta quince minutos). No se anuncia detección
instantánea del enlace radio. Las notificaciones HID permanentes se evaluarán con
hardware para evitar lectores que interfieran con software del fabricante.

## Verificación

```powershell
cargo test -p muelle -p muelle-shell -p muelle-config --locked
cargo clippy -p muelle -p muelle-shell -p muelle-config --all-targets --locked -- -D warnings
cargo build -p muelle --locked
$env:MUELLE_ACCESSORY_TEST_EXE = 'C:\Users\USUARIO\.cargo-target\muelle\debug\muelle.exe'
cargo test -p muelle --locked monitor_repeated_shutdown_and_timeout_are_bounded -- --ignored --test-threads=1
cargo test -p muelle --locked usb_process_deadline_terminates_a_stalled_provider -- --ignored --test-threads=1
cargo test -p muelle --locked accessory_indicators_render_without_controls_or_popups -- --ignored --test-threads=1
cargo test -p muelle --locked accessory_opt_out_blocks_startup_resume_and_retry -- --ignored --test-threads=1
```

Ajustar la variable al ejecutable de ese equipo. Las pruebas cubren protocolo por
trazas simuladas, comandos exclusivamente de consulta, 0/100, niveles discretos,
tramas ajenas/truncadas, interfaces desconocidas, desconexión, respuestas tardías,
backoff, señales de visibilidad y limpieza del supervisor. La prueba nativa de
UI conserva los 16 casos de escala/tema/transparencia, sin controles ni popups.

Pendiente de hardware: comparar niveles con Windows/fabricante, radio apagada,
reconexión, suspensión, dos receptores iguales y coexistencia con Options+/G HUB/GG.
No se dispone de esos dispositivos; Windows 10 requiere prueba independiente.

### Resultado local · 2026-11-19

133 pruebas normales correctas, Clippy sin advertencias y compilación optimizada
correcta. Pasaron también las pruebas nativas del supervisor, del hijo USB detenido
y de los indicadores. El hijo detenido fue terminado en unos 120 ms con un plazo
de prueba de 100 ms; no se accedió a hardware en esa simulación.

Lector optimizado con el canal de visibilidad real, tres segundos de calentamiento
y dos muestras consecutivas de cinco segundos, sin periféricos inalámbricos:

| Estado de la barra | CPU registrada | Memoria privada | Handles | Hijos USB observados |
|---|---|---|---|---|
| Visible | 0 ms | 4,027 MiB | 244 | 0 |
| Oculta | 0 ms | 4,027 MiB | 244 | 0 |

Es una muestra corta del auxiliar, con resolución limitada; no demuestra consumo
absoluto cero, coste con un receptor conectado, ausencia de fugas prolongadas ni
el consumo total del dock. La enumeración terminó correctamente con lista vacía.

Tras reducir el intervalo a 60 segundos: 133 pruebas normales, Clippy y compilación
optimizada correctos. Una nueva muestra del lector con su señal de visibilidad,
sin periféricos y tras tres segundos de calentamiento, registró 0 ms de CPU en
cinco segundos, 4,098 MiB privados y 244 handles. Es una comprobación del reposo;
no cuantifica el incremento con un receptor ni permite atribuir variaciones pequeñas
de memoria entre ejecuciones al intervalo. Ese incremento sigue pendiente de hardware.

Tras añadir el control opcional: 134 pruebas normales y Clippy correctos. La
regresión nativa comprueba el arranque apagado, activar/desactivar y que ni la
reanudación ni un reintento vencido creen de nuevo un monitor desactivado. Ajustes
y la demo Astro usan el mismo valor inicial apagado.
Compilación optimizada de dock y Ajustes correcta. La ventana real de Ajustes
guardó `true` y `false` mediante su interruptor usando un archivo temporal
independiente. La demo se comprobó en inglés/español y anchos de 390/1440 px;
`astro check` y build estático correctos. Estas pruebas no sustituyen la
validación física pendiente.

## Referencias de protocolo

Implementación propia en Rust, sin incorporar código ni binarios de otros lectores.
Se contrastaron los hechos del protocolo en estas fuentes primarias:

- [HID++ y formato de paquetes de Logitech](https://github.com/Logitech/cpg-docs/tree/master/hidpp20).
- [Identificadores de receptores en Solaar](https://github.com/pwr-Solaar/Solaar/blob/master/lib/logitech_receiver/base_usb.py).
- [Capacidad de porcentaje en HID++ 1004, controlador Linux](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-logitech-hidpp.c).
- [Identidad y estados Nova 7 documentados por su lector](https://github.com/Sapd/HeadsetControl/blob/master/lib/devices/steelseries_arctis_nova_7.hpp).
- [Obtención de informes HID en Windows](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/obtaining-hid-reports-by-user-mode-applications).
