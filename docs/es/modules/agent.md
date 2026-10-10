# Agent

Un **asistente de chat con IA** integrado: un panel de chat dentro de la app que también puede actuar sobre tus datos de OpenTraderWorld. Ábrelo desde el módulo **Agent**, el atajo de destellos de la barra superior (junto a la búsqueda) o el [botón flotante](#the-floating-assistant) situado en la esquina de cada página.

El asistente es **bring-your-own-provider**: nada está activado ni viene preconfigurado por un proveedor hasta que añadas un proveedor y una clave propios.

::: tip Dos "AI agents" distintos
Esta página trata del **asistente de chat que vive en la app** y habla con un proveedor que *tú* configuras. No es lo mismo que la página de [AI agents (MCP)](/es/config/ai-agents), que trata de agents *externos* que se conectan **a** OpenTraderWorld mediante el servidor MCP saliente. El asistente de chat puede *usar* esa misma pasarela para acceder a tus datos: consulta [Herramientas sobre tus datos](#tools-over-your-data) más abajo.
:::

## Añadir un proveedor

En **Ajustes** (el engranaje de la barra lateral del chat) → **General**, añade uno o varios proveedores. Se admiten dos formatos de transmisión:

- **Anthropic**: la API Messages de Claude.
- **Compatible con OpenAI**, es decir, cualquier endpoint que hable el formato de chat de OpenAI: OpenRouter, OpenAI, DeepSeek, Moonshot, Groq, Mistral, el endpoint de compatibilidad de Gemini, un proxy local, etcétera.

Cada proveedor tiene su propia **URL base** (solo compatible con OpenAI), **clave API** y **modelo predeterminado**. La clave es **de solo escritura**: se cifra en reposo con la clave maestra de la app y nunca se vuelve a mostrar tras guardarla. Deja el campo de la clave en blanco al editar para conservar la actual.

Un proveedor puede desactivarse sin eliminarlo. El asistente solo está "listo" cuando tiene un proveedor activado con una clave y un modelo.

## Configurar el asistente

En el mismo panel de ajustes defines el **system prompt**, el **proveedor / modelo** activo, los **max tokens** y la **temperature**. Un campo **Parámetros avanzados (JSON)** pasa tal cual al proveedor cualquier campo extra de la petición. Pon un valor en `null` para *quitar* una clave que la app enviaría de otro modo (p. ej. `max_completion_tokens` para modelos más nuevos de OpenAI, o eliminar `stream_options`).

## Chat

- Las respuestas se **emiten en streaming** y se muestran como Markdown. Los modelos que exponen su razonamiento obtienen un pliegue opcional **Thinking**.
- Las conversaciones se guardan en la **barra lateral**: nueva, seleccionar, renombrar, eliminar y **exportación a Markdown** con un clic.
- **Volcar al Editor** convierte una conversación en una página del Editor: elige una carpeta (o crea una) y un nombre de archivo, y luego **Guardar** para quedarte en el agent o **Guardar y abrir**. Cada mensaje conserva su fecha y autor (Tú, o la persona y el modelo), el razonamiento va en una cita encima de la respuesta, y **Incluir detalles** añade las llamadas a herramientas y los recuentos de tokens.
- Puedes **detener** una ejecución a mitad del streaming.
- La cabecera del chat muestra un **recuento de tokens** acumulado (entrada + salida) de la conversación, para que veas lo que cuesta un hilo.
- Un interruptor de **modo ancho** quita el límite de ancho de lectura. El hilo está dimensionado para prosa, que es la forma equivocada para las tablas que produce el asistente: un registro de pruebas o un desglose de estadísticas necesita espacio. La elección se mantiene por navegador.
- Los fallos del proveedor se leen como una frase simple en un banner descartable: una clave rechazada, un límite de tasa (con el retry-after del proveedor cuando lo envía), un modelo o URL base equivocados, un rechazo del filtro de contenido o una respuesta cortada en el límite de tokens.

### Límites del chat

Un chat puede limitarse en **tokens de salida**, en **dólares**, o en ambos. Define los valores predeterminados en **Ajustes → General → Límites de chats nuevos**; cada chat nuevo empieza con ellos, y cambiarlos después no afecta a los chats existentes. Deja un campo vacío para no tener límite.

La barra fina junto al botón de la Biblioteca de prompts en el cuadro de mensaje se llena de abajo arriba a medida que el chat gasta. Pasa el cursor para ver las cifras, haz clic para cambiar los límites de este chat. Cuando se alcanza un límite, la ejecución se detiene antes de su siguiente paso de pago y el chat no acepta mensajes nuevos hasta que subas o quites el límite, lo cual puedes hacer desde la misma barra. Enviar en el límite muestra un aviso con dos atajos: **Actualizar límite** abre ese editor, **Chat nuevo** abre un chat nuevo con la misma persona y conserva lo que escribiste.

El límite en dólares necesita un proveedor que devuelva el precio con cada respuesta (OpenRouter lo hace, Anthropic no). Sin eso el gasto aparece como "sin precio devuelto" y solo se aplica el límite de tokens.

### Cambiar de proveedor o modelo por chat

La cabecera del chat muestra el **proveedor · modelo** activo. Al abrirla obtienes una **pantalla de selección completa**: los proveedores a un lado, y la **lista de modelos en vivo** del proveedor seleccionado al otro, con búsqueda, consultada desde el servidor para que tu clave nunca llegue al navegador. El texto libre sigue funcionando para proxies que no exponen una lista. Nada cambia hasta que confirmas con **Usar este modelo**, así que explorar la lista no cuesta nada, y un único botón devuelve la conversación al valor heredado por defecto.

La elección pertenece a **esa conversación**, no al asistente: un modelo barato y rápido puede revisar tu journal en una pestaña mientras el modelo de razonamiento más potente discute un backtest en otra. Una conversación que no ha hecho elección propia hereda la de la persona, y luego lo que definas en **Ajustes → General**. Un punto en el selector marca las que van con algo propio, y un clic las devuelve al ajuste heredado. Cambiar de proveedor borra también el id del modelo, ya que un nombre de modelo solo significa algo para el fabricante del que viene.

### Enviar un prompt guardado

El compositor puede tomar de tu [Biblioteca de prompts](/es/modules/productivity#prompt-store) en lugar de reescribir un prompt que conservas. El selector lista tus prompts con una búsqueda por nombre, etiquetas y cuerpo, muestra el seleccionado completo y lo deja en el compositor con **Insertar prompt** (o doble clic en la fila), donde aún puedes editarlo antes de enviarlo.

## El asistente flotante {#the-floating-assistant}

Un botón en la **esquina inferior derecha de cada página** abre un chat compacto sobre tus conversaciones existentes, sin salir de lo que estabas haciendo. Es el mismo asistente, no uno paralelo: mismas conversaciones, personas, proveedores y herramientas, así que un hilo iniciado en la esquina está después en la página Agent y viceversa. La persona, el modelo y las herramientas están en una fila bajo el título, y los selectores de modelo y de prompt se abren como una vista sobre el hilo y no como un diálogo.

También sabe **en qué página estás**. Cada mensaje lleva el módulo actual, y cuando el token de la conversación concede ese módulo, su lista de endpoints se carga de antemano en el prompt, así que una petición hecha desde Datos históricos no gasta su primera ronda de herramientas en averiguar dónde mirar. Todo lo demás está a una consulta de distancia, y una página con la que el asistente no tiene nada que hacer (Ajustes, el dashboard) no envía nada en absoluto.

## Memoria y skills

Dos pestañas en los ajustes permiten al asistente llevar conocimiento entre conversaciones:

- **Memoria**: hechos pequeños y duraderos (una preferencia, un detalle estable) que persisten entre chats. Solo el **índice** (slug + descripción de una línea) va en el prompt; el contenido completo se obtiene bajo demanda. Tú mismo exploras, editas y eliminas cada memoria, nada está oculto. Cada memoria registra **qué persona la escribió**, mostrado tanto en el gestor como en el índice que lee el asistente: la memoria es un único almacén compartido, así que una restricción que escribió el Day Trader se leería al Analyst como propia. El asistente también puede podar memorias por sí mismo cuando el almacén se llena, y no puede sobrescribir en silencio una que escribiste a mano.
- **Skills**: conjuntos de instrucciones en Markdown reutilizables que defines. El **nombre + descripción** de una skill siempre están en contexto; el asistente carga el cuerpo completo bajo demanda cuando una tarea lo requiere. Activa/desactiva cada skill individualmente.

Las conversaciones largas también obtienen un **resumen continuo**: cuando un chat crece mucho, los turnos antiguos se comprimen en un resumen acumulado para que el hilo siga siendo barato, conservando literalmente solo los mensajes más recientes.

## Personas

Una **persona** es una versión del asistente con forma de rol: un system prompt con una postura y un límite de rechazo explícito, más una **estantería de skills** curada. Vienen cinco integradas (**Quant**, **Portfolio Manager**, **Day Trader**, **Researcher**, **Financial Analyst**) y eliges una al abrir una conversación, desde el selector de personas de la cabecera del chat.

La idea es la especialización. Un asistente genérico con doscientos endpoints es peor en cualquier trabajo concreto que uno que conoce unos pocos a fondo y rechaza el resto. El Quant no informará un backtest sin el recuento de pruebas y una cifra fuera de muestra; el Day Trader no nombrará una entrada; el Analyst informa de los fundamentales que *no pudo* obtener en lugar de rellenarlos.

### Lo que una persona no es

**Una persona no es un conjunto de permisos.** Lo que el asistente puede alcanzar es el token MCP de la conversación: acotado por módulo, definido por ti, idéntico sea cual sea la persona que hable. Cambiar de persona estrecha la *postura y la estantería*, nunca el acceso a los datos. Para cambiar lo que puede tocar, cambia el token.

### Cambiar a mitad de conversación

Puedes cambiar de persona a mitad de hilo. Surte efecto **desde el siguiente mensaje**, y aparece una marca en la transcripción que registra el relevo: los turnos por encima fueron producidos por la persona anterior y siguen atribuidos a ella.

### Editar las tuyas

**Ajustes → Personas** lista todas las personas con la estantería que realmente recibirán. Desde ahí puedes:

- **crear** una desde cero, o **duplicar** una incluida y reescribir la copia;
- editar el **prompt**, la **estantería** y si **aprueba escrituras automáticamente**;
- **restablecer** una integrada a su versión original (tus ediciones se pierden, nada más se toca);
- **eliminar** una que hiciste. Sus conversaciones se **conservan**: pasan al asistente predeterminado, y cada transcripción recibe una nota que lo indica. Las integradas no se pueden eliminar: la app las vuelve a crear en el siguiente reinicio, así que eliminarlas solo parecería funcionar.

Las skills **no** se crean aquí. Hay un único catálogo, gestionado en la pestaña Skills, y las personas eligen de él. Eso significa que editar el cuerpo de una skill lo cambia para toda persona que la tenga, y la lista de skills muestra cuántas, así que la edición nunca es a ciegas.

### Exportar e importar

Cualquier persona se exporta como un **archivo JSON con los cuerpos de las skills incluidos**, así que un solo archivo la reproduce en otra máquina. También puedes exportar una sola skill, o toda la estantería de una vez. La importación acepta cualquiera de las dos formas; un nombre existente se omite en lugar de sobrescribirse. No hay puerta de revisión: esta es tu máquina, y lo que cargues en tu propio asistente es decisión tuya.

La gestión de personas y skills está deliberadamente **ausente del catálogo MCP**: ningún agent, ni ningún contenido que un agent lea, puede editar una persona ni ampliar una estantería.

## Confirmación de escrituras

Cuando el asistente quiere cambiar tus datos, la ejecución **se pausa** y te muestra la llamada exacta (método, ruta y cuerpo) con Aprobar y Rechazar. No se escribe nada hasta que respondas, y rechazar se comunica al modelo como una negativa y no como un error que sortear. Si te marchas, la espera caduca y la escritura no ocurre.

Una persona puede configurarse para **aprobar escrituras automáticamente**, lo que omite la pregunta para cambios ordinarios. **Los borrados siempre preguntan**, diga lo que diga ese ajuste: marcar la casilla fue una decisión sobre escrituras rutinarias, no un permiso para borrar un journal.

Los endpoints que solo *calculan* (un backtest, un ratio de Sharpe, un Monte-Carlo) no preguntan. No cambian nada que echarías en falta, y un diálogo de confirmación en cada cálculo es la forma en que la gente aprende a pulsar Aprobar sin leer.

## Herramientas sobre tus datos {#tools-over-your-data}

Adjunta un **token MCP** a una conversación y el asistente puede leer y actualizar tus módulos mediante la [misma pasarela en proceso](/es/config/ai-agents) que usan los clientes MCP externos. Los **niveles de permiso por módulo** del token (Lectura / Lectura+escritura / Completo, definidos en **Ajustes → MCP**) se aplican **directamente**: el token *es* el margen de permisos; no hay una segunda puerta del lado del agent. Las operaciones de ajustes, secretos, red y borrado de datos nunca se exponen, y no hay acceso a shell ni al sistema de archivos por construcción.

Las llamadas a herramientas aparecen en línea como **chips plegables** que muestran los argumentos y el resultado. Una ejecución está limitada a 15 rondas de herramientas, y cada conversación lleva un **presupuesto de simulación**, ya que los backtests y barridos son lo único que un asistente puede gastar sin límite, de modo que cuando se agota el presupuesto se le indica que deje de buscar e informe de lo que tiene, incluido cuántas pruebas ejecutó.

### Escribir tu propia skill

Una skill es un procedimiento, no un manual. La forma que funciona:

- una **descripción** que diga *cuándo* recurrir a ella, ya que esa línea es la clave de recuperación y va en cada prompt, así que "Úsala cuando el usuario proponga una estrategia" es mejor que "Sobre estrategias";
- un **cuerpo** que nombre los endpoints exactos, paso a paso, con las formas concretas en que la tarea sale mal en esta app;
- un paso de **verificación**: cómo comprobar el resultado antes de informar;
- una **forma del informe**: lo que debe contener la respuesta.

Mantén el cuerpo corto. Llega entero a la ventana de contexto cuando se carga, así que uno largo desplaza la tarea a la que debía ayudar. El editor avisa al pasar de unas dos mil palabras.

### Herramientas por conversación

Cada conversación lleva **su propio** token MCP (el token que defines en ajustes es solo el valor predeterminado para conversaciones nuevas), intercambiable desde un **desplegable de herramientas** en la cabecera del chat. Dos conversaciones pueden ejecutarse con ámbitos de datos distintos en paralelo. El desplegable:

- tiene un **cuadro de búsqueda** para filtrar tokens y servidores externos por nombre;
- avisa cuando el token seleccionado concede **escritura/borrado**;
- ofrece acciones en línea para **añadir un servidor MCP** y accesos rápidos a **Ajustes → MCP** (crear/gestionar tokens) y a la **tienda MCP**.

## Tienda MCP: conectar plataformas externas

**Agent → Gestionar servidores** es una sección a página completa para añadir servidores MCP remotos y que el asistente pueda llegar a plataformas externas:

- un **catálogo curado** de servidores conocidos (DeepWiki, Context7, GitHub, Hugging Face, trae tu propia clave), más **servidores personalizados** por URL;
- **solo Streamable-HTTP**: nada se ejecuta nunca en local;
- los valores de autenticación están **cifrados en reposo y son de solo escritura**;
- un botón **Probar** conecta y lista las herramientas del servidor;
- activa un servidor por conversación desde el desplegable de herramientas.

Las herramientas externas llevan **espacio de nombres** (p. ej. `deepwiki__ask_question`) y se etiquetan con su servidor, las llamadas tienen tiempo limitado, y un servidor inaccesible **degrada a un aviso** en lugar de bloquear el chat.

::: warning El contenido externo no es de fiar
Un servidor MCP externo ve tu conversación, y lo que devuelve es contenido de terceros. Combinar un servidor externo con un token que concede **acceso de escritura** a tus datos significa que el contenido inyectado podría intentar provocar cambios, y el desplegable de herramientas te avisa cuando esa combinación está activa. Añade solo servidores de confianza y vigila los chips de llamadas a herramientas.
:::
