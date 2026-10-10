# Dashboard y navegación

La pantalla de inicio de la app, más las dos cosas que están por encima de cada módulo: el cuadro de búsqueda y la bandeja de notificaciones.

## Páginas del dashboard

El dashboard se abre en una página integrada de **Módulos**: un mosaico por módulo instalado, reconstruida automáticamente al instalar y desvincular. Nunca se edita ni se elimina; simplemente refleja lo que tienes.

Además, creas **tus propias páginas**. Cada una tiene un nombre, una descripción opcional y una breve **etiqueta** que se muestra en su chip. Una página es la **predeterminada**: aquella en la que se abre el dashboard y cuyo chip se ordena primero.

Úsalas como se divide una jornada de trading: una página *Mañana* con el feed de noticias, el calendario económico y la lista de verificación de la rutina; una página *Posiciones* con el portfolio y la watchlist; una página *Admin* con tareas y temporizadores.

## Editar un diseño

**Editar diseño** convierte una página en una cuadrícula de filas sobre 12 columnas. En el modo de edición puedes:

- **añadir filas** y soltar en ellas **mosaicos de módulo** (un enlace a un módulo, y el mismo módulo puede aparecer en cualquier número de páginas) o **widgets**;
- **redimensionar** cualquier mosaico por número de columnas, y arrastrar mosaicos entre filas;
- definir el **preajuste de altura** de un widget (compacto, estándar o alto) y abrir su **configuración** (el engranaje del mosaico);
- insertar **filas separadoras** para dejar respirar entre bloques.

Los mosaicos son enlaces, no copias: quitar uno de una página nunca toca el módulo ni sus datos.

## Widgets

Un widget es una vista previa viva e interactiva de un módulo: lee y escribe a través de la propia API de ese módulo, así que lo que haces en el widget es real. Los widgets cuyo módulo no está instalado simplemente no se ofrecen.

| Widget | Qué hace |
|---|---|
| **Texto libre** | Una nota o encabezado que escribes tú, markdown ligero. |
| **Feed de noticias** | Últimos elementos de un feed elegido, como lista o cuadrícula. |
| **Buzón** | El último correo sin leer, el más reciente primero. |
| **Control de Tiempo** | Inicia/detén un temporizador de proyecto sin salir de la página. |
| **Operación rápida** | Elige una categoría + plantilla y abre el formulario de nueva operación. |
| **Objetivos** | Una lista corta de objetivos con progreso; añade uno en línea. |
| **Tareas** | Tareas abiertas, marcables en el sitio. |
| **Rutina de trading** | La lista de verificación de hoy, marcable en el sitio. |
| **Mentalidad** | El check-in del día. |
| **Recordatorio** | Un formulario rápido para añadir un recordatorio. |
| **Calendario** | Hoy y esta semana de un vistazo. |
| **Calendario económico** | Próximos eventos macro, comprimidos. |
| **Portfolio** | Un resumen del portfolio con valor en vivo. |
| **Suscripciones** | Gasto recurrente mensual, y luego lo que se renueva a continuación. |
| **Patrimonio neto** | Patrimonio neto actual, su variación en una ventana que defines y un minigráfico. |
| **Watchlist** | Cotizaciones en vivo de una lista elegida: precio, variación a 24 h y 7 d. |
| **Fundamentales** | Series y tableros macro, una instantánea de empresa, una línea de estado financiero por trimestre, año o TTM, empresas ordenables, filings filtrados, próximos resultados y valoración frente a peers almacenados. Se lee de datos almacenados sin gastar cuota del proveedor. |
| **Cuant** | Datasets y backtests disponibles, riesgo y drawdown de un solo activo, correlación, estacionalidad, volatilidad realizada frente a su rango histórico y el régimen de mercado estimado. |
| **Biblioteca de prompts** | Tus prompts por etiqueta, haz clic en uno para copiarlo. |
| **Recursos** | Marcadores de una categoría elegida. |
| **Agent** | Pregunta al asistente: elige modelo y herramientas, envía y aterriza en la conversación. |

Los widgets de Fundamentales y Cuant se refrescan cada cinco minutos mientras la página es visible. Conservan su resultado anterior durante un refresco y explican una actualización fallida. Fundamentales lee solo instantáneas almacenadas; carga o actualiza los datos que falten en la página del módulo correspondiente.

En los **ajustes del widget**, elige un dataset de Cuant o una cesta de dos a veinte datasets compatibles. Los miembros de la cesta deben compartir timeframe; los miembros intradía también deben compartir proveedor. Riesgo ofrece una confianza de VaR histórico del 90 %, 95 % o 99 %, estacionalidad ofrece rendimientos, volatilidad, volumen o rango de barra, y las tarjetas de volatilidad y régimen exponen su ventana o número de estados. Cada análisis muestra su historial real y el tamaño de muestra.

Los ajustes de Fundamentales te permiten elegir y ordenar las series de un tablero macro, elegir hasta cuatro métricas de empresa o columnas de tabla, seleccionar empresas peer y filtrar filings y resultados a las empresas seguidas. El TTM de estados suma cuatro trimestres consecutivos y está disponible para las líneas de resultados y de flujo de caja; los valores del balance siguen siendo observaciones de fin de periodo. Una comparación interanual requiere el mismo periodo fiscal del año anterior y una base de comparación positiva.

Pasa el cursor, enfoca o toca una celda del heatmap para inspeccionar el valor y su recuento de muestras. Las celdas ausentes aparecen rayadas, distintas de un cero medido. Las tarjetas estrechas muestran resúmenes de estacionalidad mensual o las correlaciones de pares más fuertes. Los enlaces de los widgets abren la pestaña del módulo correspondiente con la serie, dataset o cesta seleccionados.

En pantallas de móvil de hasta 480 px de ancho, las tarjetas del dashboard se apilan en una sola columna. La disposición guardada sigue disponible en pantallas más anchas y en el editor de diseño.

## Búsqueda global

El cuadro de búsqueda de la barra superior, al que se da el foco desde cualquier sitio con <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, o simplemente <kbd>/</kbd> cuando no estás escribiendo en un campo.

Por defecto coincide con **nombres de módulos**, **secciones de Ajustes** y entradas de **Recursos**. El **interruptor de capas** junto al cuadro lo amplía a tu contenido: páginas del Editor, Objetivos, eventos del Calendario, Tareas, Rutinas, Recordatorios, Prompts y Docs de la comunidad.

Hay dos cosas que deliberadamente no hace: coincide solo con **títulos y nombres, nunca con cuerpos**, y solo busca en los módulos que tienes instalados. Los resultados vuelven agrupados por tipo, con las coincidencias por prefijo primero; <kbd>↑</kbd>/<kbd>↓</kbd> y <kbd>Enter</kbd> navegan por ellos.

## Notificaciones

La campana de la barra superior lleva un contador de no leídas y abre la **bandeja de notificaciones**, donde llegan los recordatorios de [RemindMe](/es/modules/productivity#remindme) cuando se disparan, junto con cualquier cosa que un [webhook](/es/modules/productivity#webhooks) entrante redirija allí. Una notificación que se dispara mientras estás en la app también se desliza como banner.

La entrega por **email, Telegram, Slack o Discord** pasa por los [canales de notificación](/es/config/settings#notifications) compartidos en Ajustes, donde también decides qué módulos pueden enviar a cada uno. La bandeja en sí siempre está activada y no necesita configuración.
