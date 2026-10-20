# Referencia de ajustes

Todo lo que hay bajo la entrada **Ajustes** del selector de módulos, sección por sección.

## Cuenta

Cambia tu usuario o contraseña. Se requiere tu contraseña actual para guardar los cambios, y cambiar la contraseña **cierra todas tus sesiones**.

Una contraseña nueva debe tener al menos 12 caracteres y se rechaza si aparece en listas públicas de filtraciones. Consulta [Reglas de contraseña](/es/config/security#password).

## Seguridad

Autenticación en dos factores, los navegadores con sesión iniciada en tu cuenta y cuánto tiempo puede ejecutarse una sola petición. Se cubre en detalle en [Seguridad de la cuenta](/es/config/security).

## Valores predeterminados

- **Idioma**: se aplica a toda la app de inmediato (en, fr, de, es, it, pt, zh).
- **Divisa predeterminada** y **zona horaria**: los valores iniciales que usan los módulos para nuevos elementos y para mostrar datos.

## Apariencia

El **color de acento** de la app, el que usan los botones principales, los estados activos, los enlaces y los resaltados de gráficos. Elige una muestra predefinida o cualquier color del selector; se aplica en vivo en toda la app y se guarda para todas las sesiones. *Restablecer* lo devuelve al valor predeterminado del tema.

## Red {#network}

Quién puede acceder a la app: localhost, LAN, LAN + HTTPS o público. Se cubre en detalle en [Red y acceso remoto](/es/config/network).

## Bóveda {#vault}

Un único lugar para las claves API y secretos que la app usa en tu nombre, en lugar de pegar la misma clave en cada módulo que la necesite.

Una **bóveda** representa un servicio externo (p. ej. *Binance*) y guarda **claves** con nombre: `apikey`, `secretkey`, etcétera. Crea tantas bóvedas y claves como necesites; los módulos **enchufan una clave por referencia** mediante un selector compartido, donde sea que se pida un secreto (connectors de proveedores, credenciales de feeds…).

- **Valores de solo escritura.** Un secreto se sella al guardar y nunca puede volver a verse, solo reemplazarse o eliminarse. Los *nombres* de las claves siguen visibles. Todo se cifra en reposo con la clave maestra de la app.
- **Desenchufa antes de eliminar.** Se bloquea eliminar una bóveda o una clave que siga enchufada a un módulo; quita antes la referencia allí. Cada bóveda muestra cuántas conexiones la usan.
- El **seguimiento de peticiones** es opcional y **por bóveda, no por clave**: todas las claves de una bóveda cuentan para el mismo contador. El límite es solo informativo (observar y mostrar); nunca se limita nada. Alimenta la misma vista que [Tasa de API](#api-rate).

Los secretos de los feeds de noticias también aceptan marcadores inline <code v-pre>{{vault.item}}</code>, resueltos por el planificador en el momento de la consulta. Consulta [Noticias](/es/modules/news-research#news).

## Módulos

Instala y desvincula módulos. Todo viene con la app: instalar solo hace que un módulo esté disponible en el selector y en el dashboard; no se descarga nada. Desvincular lo oculta y lo hace inaccesible; marca *eliminar también los datos* para borrar también sus datos almacenados (permanente).

## Gestionar datos

Uso de almacenamiento por módulo (tablas, filas, tamaño) con el total de la base de datos, y una acción **Borrar** para eliminar de forma permanente los datos de un módulo (escribe su nombre para confirmar). Borrar no se puede deshacer.

## Versiones {#versioning}

Dos interruptores: **Archivos del editor** y **Estrategias**, cada uno con el número de versiones almacenadas y su tamaño. Al activar uno se avisa de que cada versión es una copia completa y la base de datos crece con cada una (las imágenes y vídeos nunca se duplican). Al desactivar uno se pregunta si conservar las versiones (ocultas hasta que lo vuelvas a activar) o eliminarlas todas. Una vez activada un área, el versionado se activa por archivo o por estrategia desde su menú de historial: consulta [Editor](/es/modules/productivity#editor) y [Backtest](/es/modules/market-data#strategies-and-custom-indicators).

## Copia y restauración

Dos pestañas, cada una con un lado de **Copia** y otro de **Restauración**:

- **Completa**: comandos `pg_dump` y `psql` listos para copiar para tu despliegue, incluidas variantes cifradas, más el estado de la copia automática en las instancias que ejecutan una.
- **Parcial**: elige los módulos que quieras, descárgalos como un solo zip y cárgalo de nuevo aquí o en otra instancia. Ambos lados se cuentan primero: lo que vas a tomar, por módulo y por tabla, y lo que contiene un archivo que cargas frente a lo que ya hay aquí.

Consulta [Copia y restauración](/es/guide/backup-restore).

## Actualizar app

Muestra la versión actual, consulta GitHub por una más nueva y lista los comandos de actualización que ejecutar en el host. Consulta [Actualización](/es/guide/updating).

## Registros

El almacén de registros propio de la app, con búsqueda por mensaje/destino. El **nivel de captura** define la gravedad mínima que se escribe en el almacenamiento (efecto inmediato). Los niveles inferiores capturan más detalle y usan más espacio. Aquí puedes borrar los registros almacenados.

## Tasa de API {#api-rate}

Un dashboard de las llamadas salientes a proveedores de datos externos (datos de mercado, FX, cotizaciones, feeds), contadas por día UTC: número de peticiones por proveedor, errores, respuestas de límite de tasa, límites publicados cuando se conocen y una lista de los límites alcanzados recientemente. Existe para que veas lo cerca que estás de los límites del plan gratuito de un proveedor.

**Esta página nunca limita nada**: solo observa. El único lugar donde realmente se aplica un límite es el [límite de peticiones propio de un connector](/es/config/connectors#request-limits) en las consultas bajo demanda del gráfico; en todos los demás sitios un límite informa y avisa, y el proveedor sigue siendo quien dice que no.

## Connectors de datos

La lista compartida de cuentas de proveedores de datos de mercado usadas por Datos históricos, Visualización, Watchlists y el Trading Journal: credenciales, límites de peticiones y qué módulos pueden usar cada una. Se cubre en [Connectors de datos](/es/config/connectors). La misma pantalla está también disponible por sí sola en **/connectors**, y desde el botón de connectors dentro de cada módulo de datos.

## Brokers {#brokers}

La lista compartida de **cuentas de broker de solo lectura** usadas por el Trading Journal, Portfolios, Visualización y la Calculadora de Impuestos: credenciales, ajustes por broker y qué módulos pueden usar cada una. Se cubre en [Cuentas de broker](/es/config/brokers). Nada aquí puede colocar, modificar ni cancelar una orden.

## Notificaciones {#notifications}

La lista compartida de **canales de notificación**: adónde puede enviar la app, creados una vez y reutilizados por todos los módulos que notifican.

Un canal es un destino **que te pertenece**. Cada uno guarda un secreto, escrito aquí o enchufado desde la [Bóveda](#vault), sellado al guardar y nunca mostrado de nuevo.

| Canal | Qué aportas | Secreto | Otros campos |
|---|---|---|---|
| **Correo** | tu propio servidor SMTP | contraseña | host, puerto (587 STARTTLS, 465 TLS), remitente, destinatario, usuario |
| **Telegram** | un bot de BotFather | token del bot | id del chat |
| **Slack** | un Incoming Webhook | la URL del webhook | ninguno |
| **Discord** | un Webhook de canal | la URL del webhook | ninguno |

Los cuatro son gratuitos para el host: tú aportas la cuenta, la app no aporta nada a lo que registrarse. Algunos campos **no secretos** también aceptan un elemento de la bóveda, el **id del chat** de Telegram por ejemplo, para que un canal pueda configurarse sin que ese id quede en claro en la configuración.

Los módulos a los que se puede conceder un canal:

| Módulo | Qué envía |
|---|---|
| **RemindMe** | un recordatorio que se disparó |
| **Watchlists** | una alerta de precio |
| **Buzón** | una cuenta de correo que necesita atención |
| **Webhooks** | un payload entrante redirigido a un módulo |
| **Datos históricos** | una pausa larga y el final de un lote de descargas |
| **Visualización** | una alerta de gráfico que se disparó |
| **Journal** | el enriquecimiento con datos de mercado de una nueva operación |
| **Backtest** | un fill de paper trading, o su resumen agrupado |
| **Seguimiento de Portfolio** | se puede conceder por adelantado a lo que enviará; hoy no envía nada |
| **Automator** | lo que envíe un bloque `notify` |

- **Concesiones, por módulo.** Cada canal nombra los módulos autorizados a enviarle, o *todos*. La comprobación se hace en el servidor: un módulo al que nunca se concedió un canal no puede alcanzarlo, y el secreto de ese canal ni siquiera se descifra para él.
- **Un interruptor por canal.** Desactivar un canal lo silencia en todas partes sin eliminarlo ni borrar sus credenciales.
- **Envío de prueba** antes de depender de uno.

La misma pantalla se abre desde dentro de cada módulo que notifica, de modo que un canal puede añadirse en el momento sin salir de la página en la que estás. Está ausente a propósito del catálogo [MCP](#mcp): ningún agent puede crear un canal ni ampliar una concesión.

## MCP {#mcp}

Permite que AI agents usen la app mediante una pasarela controlada. Se cubre en [AI agents (MCP)](/es/config/ai-agents).

## Control externo {#external-control}

Maneja la app desde un canal de chat (Telegram, Slack, Discord). Se cubre en [Control externo (chat)](/es/config/external-control).

## Voz {#voice}

Comandos de pulsar para hablar y dictado: el motor de voz, los dos atajos y tus comandos de voz. Se cubre en [Control por voz](/es/config/voice). El micrófono solo funciona por HTTPS o en localhost.

## Créditos

Las fuentes de datos y proyectos originales que cada módulo puede usar, incluidos los proveedores que no has configurado.

## Acerca de

Versión, enlaces del proyecto y botones para compartir.
