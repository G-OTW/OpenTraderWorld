# Notas y organización

Los módulos del día a día: documentos, tareas, objetivos, calendario, recordatorios, más unos cuantos pensados para la disciplina del trader.

## Editor {#editor}

Un editor de documentos enriquecido al estilo Notion. Los documentos viven en un árbol de carpetas; escribe `/` para el menú de bloques.

- **Bloques**: encabezados, listas, listas de tareas, citas, bloques de código, separadores, enlaces, imágenes (subidas o por URL), color de texto, resaltado, tamaño de fuente, ancho normal o completo. Se guarda automáticamente.
- **Bases de datos**: un tipo de documento con columnas tipadas (texto, selección, URL…), visible como **tabla**, **kanban** (agrupado por una columna de selección) o **galería** (con una columna de imagen de portada). Arrastra para reordenar filas y columnas.
- **Enviar para publicación**: envía un documento a la cola de revisión de [Docs de la comunidad](/es/modules/news-research#community-docs), con el formato conservado, con idioma, categorías y crédito de autor opcional.
- **Versiones** (opcional): activa el versionado en [Ajustes → Versiones](/es/config/settings#versioning), y luego por página o base de datos desde su menú de historial. **Guardar versión** almacena una instantánea con su fecha y una nota opcional; abre una para verla en solo lectura, restaurarla (el estado restaurado se guarda como una versión nueva, anotada con la fecha de la versión restaurada) o eliminarla. El historial se abre en una ventana con una búsqueda sobre notas y fechas; la versión que coincide con el archivo actual está marcada. Las notas están limitadas a 500 caracteres. Las imágenes y vídeos no se copian: una versión apunta a las mismas subidas. Desactivar el versionado de un archivo pregunta si conservar o eliminar sus versiones. Eliminar un archivo elimina también sus versiones, tras una confirmación. Los agents con acceso de escritura al Editor pueden guardar, restaurar y eliminar versiones mediante [MCP](/es/config/ai-agents).

## Tareas {#todos}

Una lista de tareas que no estorba: tareas con fecha límite, hora, categoría y notas. Filtra por pendientes/hechas/vencidas, ordena por fecha límite; las marcas de vencida/hoy/pronto se encargan de insistir. Un widget del dashboard muestra lo que está abierto.

## Objetivos {#goals}

Objetivos con **métricas medibles**. Da a cada objetivo una fecha límite, una categoría y una o más métricas con un valor actual, un objetivo y puntos. Increméntalas a medida que avanzas, y la finalización del objetivo sigue a los puntos. Filtra abiertos/alcanzados/vencidos; arrastra para ordenar.

## Calendario {#calendar}

Un calendario personal (año/mes/semana/día) para eventos con categoría, color, ubicación y notas. Su truco son las **superposiciones**: también puede mostrar tus **Recordatorios**, **Tareas con fecha límite** y **fechas límite de Objetivos**, cada uno activable: un solo lugar para ver la semana. Crear un evento también puede crear un recordatorio sincronizado a la hora de inicio.

## RemindMe {#remindme}

Recordatorios, puntuales o recurrentes (con fecha de inicio, fecha de fin o número máximo), que se disparan como **notificaciones en la app** con una bandeja de notificaciones.

- **Recordatorios vinculados**: adjunta un recordatorio a un elemento de otro módulo (un objetivo, la facturación de una suscripción, una revisión del journal…) y enlaza de vuelta a él. La mayoría de los módulos tienen un botón *Añadir recordatorio* que lo rellena.
- **Canales**: entrega también por **email, Telegram, Slack o Discord**, elegidos de los [canales de notificación](/es/config/settings#notifications) compartidos. Un recordatorio lista los canales concedidos a RemindMe; las credenciales y las concesiones viven en Ajustes, una vez para toda la app.

## Webhooks {#webhooks}

Da a cualquier servicio externo una URL privada para **hacer POST de alertas en OpenTraderWorld**: plataformas de gráficos y alertas, notificaciones de brokers, monitores de disponibilidad, scripts, cualquier cosa que pueda lanzar una petición HTTP. El payload se recibe y se enruta a un módulo.

- **URL privada, sin cabeceras**: cada endpoint lleva un **token de 256 bits en la ruta de la URL** (`/api/hooks/<token>`), porque muchos emisores de alertas no pueden poner una cabecera `Authorization`. Los tokens se almacenan **con hash** y se muestran **una vez** al crearlos; las búsquedas fallidas se limitan.
- **Payloads liberales**: envía texto plano o JSON; el parser acepta nombres de campo laxos, así que la mayoría de emisores funcionan sin formato especial.
- **Enrutamiento**: cada endpoint redirige su payload a un módulo de destino. El destino de la v1 es **[RemindMe](#remindme)**: un payload entrante se convierte en una notificación en la app, enviada además a tus canales activados (email/Telegram/Slack/Discord).
- **Registro de entregas**: se conservan las entregas más recientes por endpoint para que confirmes que un emisor te llega y veas lo que envió.

Gestiona los endpoints en **/webhooks**.

::: warning El emisor tiene que poder alcanzarte
Un webhook solo es útil si el servicio emisor puede abrir una conexión a tu host. En el modo de red `local` (y LAN plana) nada del exterior puede, y la página te avisa cuando el modo actual no es accesible desde internet. Cambia el modo en [Ajustes → Red](/es/config/network), o apunta un túnel (p. ej. Cloudflare Tunnel) al host y mantén el resto de la app privada.
:::

## Rutinas de trading {#routines}

**Listas de verificación de sesión** recurrentes que vencen los días de la semana que elijas: preparación previa al mercado, disciplina durante la sesión, revisión posterior al mercado. Marca elementos por día, explora días pasados y mira la **tira de consistencia de 14 días** para ver si realmente sigues tu proceso. Se incluyen listas iniciales.

## Control de Tiempo {#time}

Proyectos con **temporizadores** de inicio/parada (o rangos añadidos manualmente), **presupuestos de tiempo** opcionales con avisos de exceso, fechas de fin previstas y una **tarifa horaria** para valorar el tiempo. La pestaña **Desglose** grafica las horas registradas por día/semana/mes, filtrable por proyecto y categoría. Si un temporizador se quedó en marcha mientras la app estaba cerrada, pregunta si conservar o revertir ese tiempo.

## Mentalidad {#mindset}

Un **check-in** diario para la psicología del trader. Responde unas preguntas antes o después de la sesión: escalas (foco, disciplina), opciones (calma / ansiedad / FOMO), texto libre. Las preguntas son **totalmente personalizables**; se incluye un conjunto inicial. La vista **Tendencias** grafica tus respuestas a lo largo de los check-ins recientes, y el Historial te permite releer cualquier día.

## Biblioteca de prompts {#prompt-store}

Una biblioteca para los **prompts de IA** que reutilizas: resúmenes de mercado, preguntas de journaling, plantillas de investigación. Los prompts se muestran como una cuadrícula de viñetas (nombre, etiquetas, último guardado) con una búsqueda sobre nombre, etiquetas y cuerpo.

- **Etiquetas**: añade etiquetas libres en el editor; filtra la cuadrícula con la barra de etiquetas.
- **Valorar y filtrar**: da a un prompt un pulgar arriba o abajo y filtra rápidamente por cualquiera de los dos.
- **Historial de versiones**: se conserva cada guardado; abre el **Historial** de un prompt para previsualizar cualquier revisión anterior y **volver** a ella (la restauración se guarda como una versión nueva, así que no se pierde nada).
- **Duplicar**: bifurca un prompt para crear una variante.
