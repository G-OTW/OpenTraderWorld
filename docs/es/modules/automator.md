# Automator

*Workflows que ejecutan tu propia app.* Un workflow es una lista de pasos: llamar a un endpoint de tu API de OpenTraderWorld, llamar a cualquier URL ajena a ella, hacer una pregunta a tu proveedor de IA, remodelar la respuesta, notificarte a ti mismo. Ejecútalo a mano, o con una programación.

Usos típicos: un resumen de los lunes antes de la apertura que lee los eventos económicos de la semana y tus watchlists y envía un único mensaje de Telegram; una exportación nocturna del journal a un servicio externo; un webhook de precio repartido en una notificación; un resumen semanal escrito por el asistente a partir de tus propias cifras.

El módulo vive en **/automator** con cuatro secciones: **Workflows**, **Schedules**, **Agenda** (un calendario de las ejecuciones por venir), **Runs**.

## La cuadrícula

El editor es una cuadrícula, no un lienzo: sin cables que dibujar.

- **Los pasos se ejecutan de arriba abajo, las tareas dentro de un paso de izquierda a derecha.** Todo se ejecuta una cosa tras otra. Un paso agrupa las tareas que pertenecen al mismo momento del workflow, no las inicia a la vez.
- Arrastra un bloque desde la paleta a un **hueco**: el hueco entre dos pasos abre un paso nuevo, el hueco entre dos tareas lo deja en ese paso. Todo destino de soltado válido se resalta antes de soltar.
- Una tarea solo puede leer lo que se ejecutó **antes** que ella. Mueve una tarjeta y sus referencias se comprueban de nuevo en el siguiente guardado.
- **Autoguardado** 1,2 s después de cada edición, **Ctrl/Cmd+Z** para deshacer. Un bloque aún a medio rellenar se conserva como **borrador**: nunca se ejecuta y ninguna programación lo recoge hasta que valida.

## Bloques

| Bloque | Qué hace |
|---|---|
| **App call** | Un endpoint de tu propia API, ejecutado en proceso. La paleta lista los endpoints que un workflow puede alcanzar; haz clic en uno y el bloque aterriza ya apuntando a él, con la forma esperada del cuerpo a mano. |
| **Web call** | Cualquier URL ajena a la app: método, query, cabeceras, cuerpo JSON/texto/formulario, respuesta leída como JSON, texto, CSV o binario, con un límite de tamaño de respuesta. |
| **AI step** | Un turno de modelo, sin conversación alrededor. Elige una persona y un prompt almacenado o escribe las instrucciones; pide un **objeto JSON** cuando el siguiente bloque tiene que leer campos y no prosa. Llama a tu proveedor y cuesta tokens en cada ejecución. |
| **Transform** | Remodela lo anterior: `pick` un valor, `set` un objeto, `format` una cadena, `csv_parse` un CSV, `join` una lista en una línea. |
| **Notify** | Una notificación en la app y tus [canales de notificación](/es/config/settings#notifications) (email, Telegram, Slack, Discord). Sin nada seleccionado significa todos los canales activados y concedidos al Automator. |
| **Wait** | Pausa la ejecución. **Stop** sigue respondiendo durante la espera. |

## Pasar datos entre bloques

Cada bloque tiene un **id**, mostrado en la parte superior de su editor. Un bloque posterior lee su resultado con una expresión:

```
{{steps.http1.output}}                     the whole answer
{{steps.http1.output.items.0.name}}        one field of it
{{steps.http1.status}}                     ok | simulated | failed | skipped
{{steps.http1.error}}                      the failure message, empty on success
{{run.started_at}} {{run.trigger}} {{workflow.name}} {{input.key}}
```

Los filtros se encadenan tras una barra vertical: `json`, `upper`, `lower`, `trim`, `round:2`, `date:"DD/MM/YYYY"`, `default:"n/a"`. Solo `default` rescata un valor que no existe.

Esto **no es un lenguaje**: sin aritmética, sin código. Cada referencia se comprueba **al guardar el grafo**, contra los bloques que realmente la preceden, así que un enlace roto se rechaza en el editor y no a las tres de la madrugada.

## Secretos

Una contraseña o una clave API pertenece a la [Bóveda](/es/config/settings#vault), nunca se escribe en un campo. Referénciala con <code v-pre>{{vault.myvault.mykey}}</code> en una cabecera, un valor de query o un cuerpo de petición, los únicos lugares donde se acepta (nunca en una URL), y el valor resuelto se borra del historial de ejecuciones. Un valor de query sigue llegando a los registros del sitio al que llamas, así que prefiere una cabecera.

## Permisos

- **App call necesita un token de acceso.** Elige uno en los **Ajustes** del workflow; los tokens se crean en [Ajustes → AI agents](/es/config/ai-agents). Sin token no hay ninguna llamada interna, y un workflow solo puede alcanzar lo que su token concede, dentro de la misma lista de permitidos que usa la pasarela MCP.
- **Web call rechaza tu propia red.** Las direcciones loopback, privadas, CGNAT y link-local se rechazan salvo que el bloque permita explícitamente destinos internos. El host se resuelve primero y la conexión queda fijada a la dirección que se comprobó, y cada salto de redirección se comprueba de nuevo.
- **Notify necesita una concesión.** Un canal solo se ofrece aquí después de que el Automator lo haya recibido en Ajustes → Notificaciones.

## Probarlo y luego ejecutarlo

- **Test run** realiza las lecturas e informa de lo que habría hecho una escritura o un envío, así que nada sale de la app. Un bloque que lee un valor que solo una escritura simulada podría haber producido se informa a su vez como **simulated**, en lugar de fallar una prueba que una ejecución real superaría.
- **Test this block** ejecuta un solo bloque aislado, mismas reglas.
- **Run now** lo hace de verdad. **Stop** se comprueba entre bloques y durante una espera; una ejecución que supera el **límite de ejecución** del workflow se cierra como timeout.

## Runs

Cada ejecución conserva **una línea por bloque**: lo que se envió, lo que volvió, el estado y los tiempos, así que un workflow que falló a las 3 a. m. nombra el bloque y muestra el payload. Una salida de más de 256 KB se aparca y se obtiene bajo demanda. El historial se recorta a las últimas 50 ejecuciones por workflow (10 para ejecuciones de prueba).

## Schedules

Cada N minutos, a diario, algunos días de la semana, mensualmente o una sola vez en un momento dado, cada una con su propia **zona horaria IANA**, de modo que una regla definida en Europe/Paris sigue a París y no al servidor.

- **Sin solapamiento**: si una ejecución sigue en marcha cuando vence la siguiente ocurrencia, esa ocurrencia se **descarta**, no se encola.
- **Recuperar** (opcional): si la app estaba caída a la hora prevista, ejecutar una vez al arrancar.
- El horario de verano se resuelve, no se ignora: una hora local omitida se ejecuta al final del hueco, una duplicada se ejecuta una vez. Una regla mensual definida más allá del final de un mes corto se ejecuta en su último día.
- Una programación puede **fijarse a una versión** del workflow. Al guardar un grafo nuevo se pregunta si las programaciones fijadas deben seguirlo; el autoguardado nunca reapunta una por sí solo.
- **Pausar / reanudar** desde la tarjeta del workflow o la lista de Schedules. Un workflow desactivado nunca lo inicia una programación, aunque ejecutarlo a mano sigue funcionando.

::: warning Una escritura programada escribe
Un App call apuntado a un endpoint que cambia tus datos lo hará en cada ejecución, sin supervisión. El editor marca esos endpoints; prueba el workflow antes de programarlo.
:::

## Dejar que un agent construya un workflow {#letting-an-agent-build-a-workflow}

Componer un grafo es lo más difícil que te pide este módulo, y es exactamente el tipo
de trabajo en el que un [AI agent](/es/config/ai-agents) es bueno. Así que un agent con un token con el
permiso de **Automator** puede leer tus workflows, crear uno, escribir su grafo y probarlo.

Lo que no puede hacer es ponerlo en servicio. La separación es deliberada:

- Un grafo que un agent guarda aterriza como **borrador**, nunca como el grafo que se ejecuta. Abre el
  workflow, lee lo que escribió y Guarda para adoptarlo. Hasta que lo hagas, nada cambia: un
  workflow que ya está programado sigue ejecutando la versión que guardaste.
- Un agent no puede asociar el **token de acceso** de un workflow. Ese margen es lo que convierte un grafo
  en permisos, así que lo concedes tú a mano, después de leer el grafo que va a ejecutar.
- Un agent no puede **ejecutar** un workflow, restaurar una revisión, eliminar uno ni tocar una programación.
- Sus **ejecuciones de prueba** están selladas: las notificaciones y las llamadas externas se fuerzan a desactivadas, y sin
  margen asociado un App call falla en modo cerrado. Una prueba comprueba que el grafo se ejecuta, que sus
  expresiones se resuelven y que sus transforms hacen lo que dicen, sin llegar a nada. Un
  workflow que ya lleva un token se rechaza: ese lo pruebas tú.

La razón de la línea es que un workflow se ejecuta bajo **su propio** token, no el del llamante.
Un agent capaz de escribir un grafo y de iniciarlo heredaría lo que conceda ese token,
digan lo que digan sus propios permisos. Escribir y armar son dos concesiones, y solo una de ellas
es tuya para delegar.

El Automator está desactivado en el [modo demo](/es/guide/demo).
