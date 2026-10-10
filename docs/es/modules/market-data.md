# Datos de mercado y backtesting

Estos módulos forman una cadena: **Datos históricos** descarga el historial de precios a datasets locales; **Backtest** y **Herramientas Cuant** trabajan sobre esos datasets, y **Visualización** grafica cualquier instrumento que sirva un connector, esté almacenado o no. Los tres requieren que Datos históricos esté instalado, ya que es quien posee el catálogo de datasets que leen.

## Datos históricos {#histdata}

Descarga velas OHLCV de proveedores externos a datasets almacenados en tu base de datos, o [importa un archivo](#import) que ya tengas. Una vez almacenados, los datos son tuyos: grafícalos, haz backtest, expórtalos, sin volver a obtenerlos.

### Proveedores y credenciales

Los proveedores se configuran una vez, de forma centralizada, como **[connectors de datos](/es/config/connectors)**: una cuenta de proveedor con nombre, con sus credenciales, un límite de peticiones opcional y los módulos autorizados a usarla. Datos históricos **no tiene ajustes de proveedor propios**: el botón *Connectors* junto al selector de proveedor abre la misma pantalla compartida que encontrarías en Ajustes.

Algunos proveedores **no requieren clave** (Binance y Binance futures, Bitget, OKX, Kraken, Coinbase, Yahoo Finance) y funcionan de inmediato; otros necesitan una clave API, y la mayoría tiene plan gratuito. Un connector al que le faltan sus credenciales muestra *necesita credenciales* y se omite hasta que las definas.

Las llamadas salientes se cuentan en **Ajustes → Tasa de API** para que puedas vigilar el uso de tu plan gratuito.

### Descargar

Elige proveedor, tipo de activo, timeframe, ticker y rango de fechas, y luego **Descargar**. Notas:

- **Los futuros** usan códigos de contrato: base + letra del mes + dígito del año (`F G H J K M N Q U V X Z` = ene…dic), p. ej. `GCJ5` para el oro de abril de 2025.
- **Las opciones** se construyen a partir del subyacente, el vencimiento, call/put y el strike.
- **Límites intradía**: los proveedores solo sirven granularidad intradía para un retroceso limitado (p. ej. ~7, 60 o 730 días según el proveedor). El historial más antiguo está disponible en **1d / 1w** sin límite. El formulario te avisa antes de encolar un rango imposible.

::: warning Datasets descargados antes de la v0.0.15
Antes de la v0.0.15, una descarga o actualización que llegaba a la hora actual podía almacenar la vela aún en formación, y las actualizaciones posteriores empezaban después de ella. Un dataset así puede contener **velas incompletas** (máximo, mínimo, cierre y volumen truncados). Descárgalo de nuevo para reemplazarlas. Desde la v0.0.15, una vela solo se almacena cuando su periodo ha terminado.

| Proveedor | Datasets que pueden contener velas incompletas |
|---|---|
| Coinbase, Kraken, Yahoo Finance, Alpaca, Massive, EODHD, Alpha Vantage, Capital.com, Interactive Brokers | descargados o actualizados antes de la v0.0.15 |
| Binance (spot) | descargados o actualizados antes de la v0.0.12 |
| Binance USDⓈ-M, Bitget, OKX, OANDA, TradeStation, FOREX.com | ninguno |
:::

### Varios a la vez

Un solo formulario encola un lote entero: marca **tantos timeframes** como necesites y escribe **varios tickers separados por comas** (`BTCUSDT, ETHUSDT, SOLUSDT`). Se encola una descarga por cada par símbolo × timeframe (3 símbolos × 2 timeframes = 6 descargas), todas sobre el mismo connector y el mismo rango de fechas.

Antes de pulsar Descargar, el formulario **valora el lote**: cuántas descargas son, aproximadamente cuántas peticiones al proveedor cuesta y el tiempo mínimo que tardará (se ejecutan una tras otra para respetar los límites de tasa del proveedor). Cuando el connector lleva un [límite de peticiones](/es/config/connectors#request-limits), un pequeño indicador muestra cuánto de la ventana actual ya está gastado, y la línea te avisa cuando el lote lo supera. No se bloquea: el resto espera a que se restablezca la cuota y se reanuda por sí solo.

### Vigilar los jobs

Las descargas se ejecutan como **jobs** en segundo plano, fragmento a fragmento, con progreso en vivo. Filtra jobs por estado, proveedor, timeframe o ticker; un lote se agrupa bajo una cabecera que muestra cuántas de sus descargas están hechas.

- **Un job que alcanzó un límite está en `waiting`, no fallido.** La fila dice por qué (*cuota alcanzada* o *límite de tasa del proveedor*) y cuenta atrás hasta el momento en que se reanuda solo.
- **Cancelar** detiene cualquier job sin terminar, y un clic cancela **el resto de un lote**. Cancelar es cooperativo: el worker se detiene en el siguiente límite de fragmento y las barras ya escritas se conservan.
- Una pausa larga y el final de un lote generan una notificación, enviada a los [canales](/es/config/settings#notifications) concedidos a Datos históricos.

### Datasets

La pestaña **Datasets** lista todo lo almacenado: número de barras, rango de fechas, tamaño. Desde aquí puedes:

- **Obtener más recientes**: traer barras más recientes que la última almacenada (completar un dataset).
- **Exportar** como **CSV** o **Parquet**. El CSV se abre en cualquier hoja de cálculo; Parquet son las mismas barras tipadas y comprimidas, aproximadamente una décima parte del tamaño, leído por `pd.read_parquet` sin análisis de fechas ni adivinar dtypes. El archivo Parquet también lleva el instrumento, el timeframe y la fuente en sus propios metadatos, así que al importarlo de vuelta en cualquier parte de la app rellena el formulario solo.
- **Eliminar** un dataset (descarta todas sus barras).
- Saltar directamente a un **gráfico** de él.

Las descargas de Capital.com y OANDA también almacenan el **bid y el ask** de cada vela; para los demás proveedores un backtest los obtiene cuando [los necesita](#bid-ask-providers).

Los datasets importados están en la misma lista, con el nombre del lugar de donde vino su archivo y no el de un proveedor. No llevan botón *Obtener más recientes*: no hay proveedor detrás, y la forma de ampliar uno es otro archivo.

### Importar tu propio archivo {#import}

**Importar** en la pestaña Datasets lee un historial de precios que ya tengas: un volcado de un exchange, una exportación de broker, una hoja de cálculo, el archivo de un proveedor. CSV, TSV, TXT, JSON o **Parquet**, hasta 20 MB, una fila por barra.

El archivo nunca sale de tu navegador entre pasos y nunca se almacena en el servidor: cada paso lo envía de nuevo, así que no hay una subida a medias que reanudar ni limpiar.

**Las columnas se proponen, tú las confirmas.** Las cabeceras se emparejan con un diccionario multilingüe (seis idiomas) *y* con el aspecto real de los valores, así que `Date;Ouverture;Plus haut;…` y `open_time,open,high,low,close,volume` quedan mapeadas. Un punto junto a cada columna indica lo seguro que está el detector; todo lo que le genera dudas queda para ti. Corregir una columna le enseña: el siguiente archivo con esa cabecera se mapea solo.

Un archivo Parquet se lee en la misma cuadrícula que un CSV, así que la detección de columnas, el paso de mapeo y la vista previa funcionan igual. Se respetan sus tipos: un timestamp INT96 heredado (lo que escriben Spark y pandas antiguos) y un `DATE` se convierten en fechas, un `DECIMAL` conserva su escala, un nulo sigue siendo una celda vacía. El mismo lector sirve a las importaciones del Journal y del Portfolio, así que esas también aceptan Parquet.

Los **timestamps** se leen como fechas o como enteros Unix en segundos, milisegundos, microsegundos o nanosegundos, detectados por columna y modificables. Un timestamp de texto que no lleve zona horaria se lee con el offset que elijas, que decide *a qué periodo* pertenece cada fila, no solo cómo se muestra.

**Lo que falta se rellena, nunca se inventa.** Un archivo con una sola columna de precio es una serie de cierres (un NAV, un nivel de índice): apertura, máximo y mínimo se rellenan con el cierre, formando una barra plana, y el formulario lo dice. Una columna que *sí* mapeaste y está vacía en una fila es un error que nombra su línea, no un cero.

Antes de escribir nada, la vista previa informa sobre todo el archivo: barras, filas, columnas, la primera y la última fecha, el espaciado que tienen realmente tus timestamps (ofrecido como timeframe), periodos que faltan a ese espaciado, filas que comparten un periodo, barras cuyo máximo/mínimo no contienen su apertura/cierre y cada fila que no se pudo leer.

**Lo que el archivo no puede decir, lo dices tú.** Un archivo dice "Close"; no dice que las barras sean AAPL diario. Por eso la importación pide:

- **Ticker**, **tipo de activo** y **timeframe** (el timeframe viene rellenado a partir del propio espaciado del archivo).
- **Fuente**: el broker, plataforma o proveedor de donde vino el archivo, texto libre. Es *parte de la identidad de la serie*, de modo que el mismo instrumento exportado por dos brokers sigue siendo dos datasets en lugar de dos cintas promediadas en una.
- **Nombre** y **etiquetas**: tus propias etiquetas, usadas para encontrar la serie de nuevo en el catálogo y filtrarla.

**Importar dos veces es seguro.** Una reimportación aterriza en el mismo dataset y sobrescribe periodo a periodo: mismo archivo, mismo resultado. Los periodos solapados se cuentan en la vista previa antes de confirmar.

Un archivo Parquet exportado desde aquí se salta la mayor parte de ese formulario: ya conoce su ticker, tipo de activo, timeframe y fuente, y solo rellena las casillas que dejaste vacías, así que lo que escribas sigue prevaleciendo. Exporta, edita en pandas, importa de vuelta.

Una vez importada, la serie es un dataset normal: los backtests, las Herramientas Cuant, el enriquecimiento del journal y los proxies de factores del portfolio la leen como cualquier descargada.

### Mira antes de descargar

No tienes que encolar un job para averiguar si merece la pena almacenar un símbolo. El gráfico obtiene una ventana mediante un connector y **no almacena nada**; cuando la ventana parece correcta, **guárdala** y se encola el job de descarga normal para exactamente ese rango. Guardar sobre barras que ya tienes consolida en lugar de duplicar, así que completar un dataset desde el gráfico es seguro.

## Visualización de datos históricos {#histviz}

El gráfico no está atado a un dataset: abre un **instrumento**. Busca un símbolo, elige un timeframe, y las barras llegan las hayas descargado o no: el servidor sirve lo que ya está en tu catálogo y obtiene solo los extremos que faltan mediante un [connector](/es/config/connectors). No se escribe nada a menos que lo pidas.

La página es un **espacio de trabajo**: una cuadrícula de gráficos, una lista de instrumentos a su lado y una sesión de backtest rápido debajo. Todo lo que sigue describe un solo gráfico salvo que se diga otra cosa; la cuadrícula en sí está en [Espacios de trabajo](#workspaces).

### Encontrar un instrumento

No se escribe nada sobre las velas que no les pertenezca: el **símbolo de arriba a la izquierda de un gráfico es un botón**, y abre el selector de instrumentos como modal, un cuadro de búsqueda sobre todos los connectors que el gráfico puede usar. Escribe `BTC` y Binance, Bitget, OKX, Kraken, Coinbase, Yahoo, EODHD, Alpha Vantage, Alpaca y Massive responden juntos, cada resultado etiquetado con el connector que lo sirvió. Filtra por tipo de activo; marca o desmarca fuentes en el mismo modal (la marca **es** la concesión, y el servidor rechaza un connector que este módulo nunca recibió).

Con el cuadro vacío, el panel lista lo que graficaste **recientemente**, luego lo que ya está **almacenado**, ambos abribles con un clic y sin coste alguno.

Una fila que no puedes graficar lo dice en lugar del gráfico, nombrando el motivo: el connector nunca se concedió, el proveedor no conoce el símbolo, falta una credencial, o ningún connector concedido lo sirve. Cada mensaje lleva el botón que lo arregla.

### Espacios de trabajo {#workspaces}

Un espacio de trabajo es una **cuadrícula de gráficos**, de 1x1 hasta 3x4. Las filas y columnas se eligen desde la barra de herramientas, así que una división vertical, una horizontal y una 2x2 son el mismo control y no una lista de diseños con nombre; arrastra los separadores para dar más espacio a un gráfico, o maximiza un gráfico y vuelve a la cuadrícula. Conserva tantos espacios de trabajo como quieras, ponles nombre y cambia desde el selector; el que tenías abierto vuelve al recargar.

Cada gráfico lleva su propio instrumento, timeframe, estilo de trazado, indicadores y dibujos. Un gráfico se cierra desde su propia cabecera, y una celda vacía pide un instrumento.

**Grupos de enlace.** Haz clic en el botón de enlace de un gráfico para darle un color. Los gráficos que comparten color comparten el **símbolo** y el **crosshair**, y también el tramo visible cuando están en el mismo timeframe. El timeframe en sí deliberadamente nunca se comparte: tres paneles sobre un símbolo en 1m, 1h y 1d es la razón para enlazarlos.

**Una conexión para todos.** Los paneles de la misma cuenta comparten un único stream en vivo, así que cuatro gráficos con una clave de Alpaca gastan un asiento de conexión, no cuatro.

### La lista de instrumentos {#rail}

Un carril a la izquierda de la página, de dos fuentes que nunca se mezclan:

- Las **listas de gráficos** son propias del carril, creadas con el botón `+`, que abre el mismo selector de instrumentos. Contienen coordenadas de gráfico, así que un clic las grafica sin búsqueda. **Recientes** es lo mismo sin nombre.
- Las **listas del módulo Watchlists** contienen símbolos de cotización, así que graficar uno es una búsqueda. Cuando la lista o la fila cotiza mediante un connector de datos, ese connector es el único consultado: un símbolo cotizado mediante IBKR se grafica en IBKR o no se grafica.

Haz clic en una fila para graficarla en el panel activo, o arrástrala a cualquier panel. **Nunca se escribe en una watchlist desde aquí**: editar una la copia primero a una lista de gráficos y edita la copia, y convertir una lista de gráficos en una watchlist real es el botón **Promocionar** y nada más.

### Cargar historial

El gráfico se abre con las **1500 barras** más recientes y pone un botón en el borde izquierdo de los datos cargados. Cada clic retrocede un tramo más. Nunca se hace una petición al proveedor sin un gesto tuyo, lo que mantiene predecible una clave medida; el recorrido se detiene cuando se acaba el historial del proveedor.

El desplegable de **timeframe** ofrece cada tamaño de barra que admite el connector, no solo los que hayas descargado, y cambiar de timeframe **conserva las fechas que estabas mirando**.

### Streaming en vivo {#live}

Para un connector con capacidad de stream, el gráfico **se pone en vivo solo** en cuanto la barra más reciente
es la que se está formando ahora: la última vela se actualiza en el sitio, y el control muestra el estado de la conexión
y el retraso respecto al exchange. El mismo control detiene y reinicia el feed a mano.

El live se dirige por **instrumento**, no por dataset, y **no almacena nada**. Cualquier símbolo que sirva un proveedor
de streaming puede verse en vivo sin descargarlo antes, que es el objetivo:
puedes mirar algo antes de decidir si merece conservarse. Guarda el instrumento mientras está
en vivo y el mismo feed empieza a grabar también barras cerradas en el dataset.

**Quién emite qué.** Binance (spot y futuros), Bitget, OKX, Kraken y Coinbase emiten todos los
timeframes que descargan, el diario incluido, porque en un mercado 24/7 la vela diaria es el día
de época. Capital.com también emite todos los timeframes, a partir de las velas bid y ask que publica
por separado, graficadas en su punto medio. Alpaca, Massive e
Interactive Brokers publican **un solo grano cada uno** (barras de un minuto, agregados de un minuto,
barras de cinco segundos) y el gráfico construye tu timeframe a partir de él. Eso cubre todos los timeframes
**intradía** y deja el diario y el semanal a la descarga: una sesión de acciones no son 1440
minutos alineados a la época, así que una vela diaria construida así no coincidiría con la almacenada. En
un timeframe que no puede emitir, el control dice cuáles sí pueden en lugar de desaparecer. El
detalle por proveedor está en [streaming en vivo](/es/config/connectors#live-streaming).

Un gráfico emite un símbolo sea cual sea el número de paneles: varios paneles sobre un instrumento, y
varios paneles sobre una cuenta, comparten la conexión en lugar de ocupar cada uno un asiento.

**Qué cuenta.** Cuando un proveedor tiene más de un connector, el control en vivo gana un selector de
cuenta. Dos claves son dos derechos y dos asientos de conexión, así que cuál se gasta
es decisión tuya, no un respaldo. Cambiar reinicia el feed en la otra.

#### Sin hueco en la unión

Cargar la ventana, abrir el socket y esperar a que el proveedor publique llevan tiempo,
y un proveedor de barras de un minuto solo habla una vez por minuto. Cuando llega el primer tick en vivo, el
gráfico puede ir una o varias velas por detrás, y esas velas solían faltar hasta que recargabas.

Así que el stream le dice al servidor dónde acaba el gráfico, y el servidor envía lo que cerró
entretanto: del catálogo cuando el instrumento está almacenado, del proveedor solo para la cola que
no puede responder, y nada en absoluto cuando no hay hueco. Lo que ves al ponerte en vivo es lo que hizo el mercado,
sin un agujero en la unión.

#### Cuando no puede funcionar

Una clave rechazada, un plan sin streaming, un instrumento para el que tu cuenta no tiene suscripción:
son respuestas, no fallos. El gráfico nombra cuál es, cita las palabras del propio
proveedor y **se detiene** en lugar de reconectar para siempre tras un punto que nunca se pone verde.

| Lo que ves | Qué significa | Qué hacer |
|---|---|---|
| *Not authorized* / clave rechazada | las credenciales son incorrectas, o el plan no tiene feed en vivo (una clave gratuita de Massive descarga historial y es rechazada en el login en vivo) | arregla el connector y pulsa *Reintentar* |
| *No subscription* para este símbolo | la cuenta está conectada pero sin derecho al feed de ese instrumento (Alpaca gratuito emite IEX, no SIP ni OPRA; IBKR sirve lo que contratas) | elige otro instrumento o añade la suscripción en el proveedor |
| *Connection taken* | la mayoría de proveedores permiten una conexión en vivo por cuenta, y otro programa ocupa el asiento | cierra el otro programa; este sigue reintentando solo, ya que se resuelve por sí mismo |
| *This timeframe does not stream* | el proveedor publica un solo grano y tu timeframe está por encima | cambia a uno de los timeframes que lista el control, o quédate con los datos descargados |
| *Connector not granted* | nunca se dio este connector al gráfico | concédelo en [Connectors de datos](/es/config/connectors), desde el enlace del mensaje |
| *Market data lines* casi agotadas | Interactive Brokers limita cuántos símbolos emite una cuenta a la vez, y el espacio de trabajo está cerca de ese tope | cierra un panel, o detén el feed en vivo de uno que no estés mirando, antes de que el siguiente gráfico se quede mudo sin dar razón |

Todo lo demás (un socket caído, un contratiempo del proveedor) se reconecta en silencio con un backoff.

### Gráfico

- **Tipos de gráfico**: velas, barras OHLC, línea y **Renko** (con tamaño de ladrillo).
- **Indicadores**: SMA, RSI, MACD y más, como superposiciones o paneles separados, cada uno con fuente, colores de línea/relleno y grosor configurables. Cada serie tiene **su propia línea en la cabecera del gráfico**, con ocultar, ajustes y quitar al pasar el cursor, y cada panel lleva su título sobre el dibujo que contiene. La cabecera **lee en el crosshair**: O H L C, la variación y el valor de cada indicador en la barra bajo el cursor, recurriendo a la barra visible más reciente cuando el cursor está fuera.
- **Ajustes del gráfico**: escala lineal o logarítmica, líneas de cuadrícula horizontales y verticales, **separadores de día**, el **cierre anterior** dibujado como línea de referencia, el crosshair y sus etiquetas de valor por serie, tooltip al pasar el cursor (desactivado por defecto), colores de subida/bajada, escala de precio a la izquierda o a la derecha, y una **etiqueta de último precio** fijada en el eje de precios, teñida como la vela que la produjo.
- **La navegación es manual**: arrastra para desplazar ambos ejes (el tiempo hacia los lados, el precio arriba y abajo), rueda para hacer zoom (calibrada por dispositivo, de modo que un tope del ratón y un tick del trackpad mueven lo mismo). Cambiar el tipo de gráfico conserva el zoom actual.
- El **volumen** se dibuja en la parte inferior del panel de precios, como lo dibujan los terminales de mercado, y no en un panel propio. Más un único modo de pantalla completa.
- Los **precios de micro-cap** se escriben `0.0₅4549`, con el subíndice contando los ceros, en lugar de una escala de etiquetas idénticas `0.0000`.

### Comparar dos instrumentos

Añade otro instrumento al mismo gráfico y se dibuja **rebasado**, ya que dos precios en dos divisas en un mismo eje no dicen nada. Dos lecturas, a un clic de distancia:

- **variación porcentual**, ambas series rebasadas al inicio de la ventana, que responde *cuál subió más*;
- **ratio**, este instrumento dividido por el otro, rebasado a 100, que es la vista de pair trade: la línea sube cuando el que estás graficando rinde más que el otro.

Las series de comparación se alinean con el gráfico **periodo a periodo**, así que dos mercados que marcan el mismo día de forma distinta siguen encajando.

### Guardar el gráfico

- **Como imagen**: un PNG del gráfico exactamente como está en pantalla, con dibujos y superposiciones incluidos.
- **Como página**: un archivo HTML que se abre sin conexión en cualquier navegador, con la imagen, de qué es una imagen (instrumento, ventana, timeframe, indicadores, comparaciones, quién sirvió las barras) y **las propias barras**, incrustadas. Una captura pegada en un documento es una afirmación que nadie puede comprobar después; esta se puede releer. No se sube nada: el archivo se construye en tu navegador.

### Indicadores personalizados en el gráfico {#custom-indicators-on-the-chart}

El diálogo de indicadores tiene una segunda pestaña: **Personalizados**. Contiene la misma biblioteca de grafos de nodos con la que construye el módulo [Backtest](#strategies-and-custom-indicators), así que un indicador existe **una vez** y ambos módulos ven la misma definición. Elige uno de la lista para trazarlo, o construye uno nuevo aquí mismo con el mismo constructor; al guardar se escribe de vuelta en la biblioteca compartida.

A diferencia de un indicador del catálogo, uno personalizado **elige su propio panel**: sobre el precio, o en un panel propio. Esa elección es tuya por instancia, así que el mismo indicador puede superponerse a las velas en un gráfico y situarse debajo en otro. Un indicador 0-100 dibujado como superposición recibe una segunda escala oculta, para que no pueda aplastar el precio.

La definición **viaja con la instancia**: un gráfico sigue dibujando su indicador personalizado después de eliminar la fila de la biblioteca, y recargar lo actualiza desde la biblioteca mientras la fila exista.

### Herramientas de dibujo

Un carril contra el borde izquierdo del gráfico: **línea de tendencia**, línea **horizontal** y **vertical**, **rectángulo**, **retroceso de Fibonacci**, **texto**, cajas de posición **larga** y **corta** (entrada, objetivo y stop, con el R:R resultante) y una **medida** que informa del cambio de precio, el porcentaje, el número de barras y el tiempo transcurrido.

- Cada objeto tiene su propio **estilo** (color, grosor, guiones) y se edita arrastrando sus manejadores.
- Un **imán OHLC** ajusta un manejador a la apertura, máximo, mínimo o cierre de la barra bajo él, y un manejador soltado cerca de un objeto ya presente en el gráfico se ajusta a él, con una línea guía que dice qué capturó.
- **Plantillas de estilo**: da estilo a un objeto, guárdalo con un nombre y aplícalo a los siguientes. Una plantilla puede ser la predeterminada para todo dibujo nuevo.
- **Copiar entre gráficos**: <kbd>Ctrl/⌘+C</kbd> y luego <kbd>Ctrl/⌘+V</kbd> pega el objeto seleccionado, también en otro instrumento; <kbd>Ctrl/⌘+D</kbd> lo duplica en el sitio, desplazado una barra.
- Los dibujos están anclados en **tiempo y precio**, no en píxeles, así que se quedan sobre sus barras con cualquier zoom, desplazamiento o cambio de timeframe.
- Se conservan **por instrumento**, no por timeframe ni por panel: una línea de tendencia dibujada en el 1h es la misma línea en el 15m, y dos paneles sobre un símbolo muestran un solo tablero.
- **Deshacer** (el botón del carril, o <kbd>Ctrl/⌘+Z</kbd>) retira el último dibujo, o la última orden del backtest rápido.

#### La lista de objetos

A partir de cinco dibujos un gráfico necesita una lista, así que hay una: cada objeto de este instrumento con lo que es y el precio en el que está, y las cuatro cosas que luego necesita, **ocultar**, **bloquear**, **eliminar** y **reordenar**. El orden es el orden de pintado, que decide qué queda encima. Seleccionar una fila la selecciona en el gráfico, y la lista es el mismo array que dibuja el gráfico, así que una edición se ve antes de cerrar el diálogo.

### Alertas {#alerts}

Un precio o un nivel de indicador, **vigilado por el servidor**. El navegador puede estar cerrado, la máquina puede estar haciendo otra cosa: la alerta se dispara igualmente, en tus notificaciones y en los [canales](/es/config/settings#notifications) concedidos al gráfico (ninguno marcado = todos).

Defínela desde el diálogo de alertas, o desde una línea horizontal que ya hayas dibujado, que entrega su precio.

Hay tres reglas que conviene conocer, porque son decisiones y no detalles:

- **Solo barras cerradas.** El máximo de una vela en formación aún no es un hecho, el siguiente tick puede revisarlo. Una alerta que se disparara con él informaría de algo que nunca ocurrió.
- **Un cruce, no un estado.** *Cruza al alza* espera a que el precio **atraviese** el nivel subiendo, así que una alerta colocada por debajo del precio actual no se dispara en el instante de crearla.
- **El nivel se lee en el timeframe de este gráfico**, y una alerta diaria se relee mucho menos a menudo que una de un minuto: un instrumento no almacenado cuesta una pequeña petición por comprobación, y una barra que se mueve una vez al día no merece una por minuto.

Cada alerta puede dispararse **una vez** o cada vez, con un cooldown. La lista dice cuándo se disparó cada una por última vez, qué leyó por última vez y, cuando un connector o una cuota se interponen, por qué no pudo ejecutarse. Las alertas se pausan y rearman desde esa lista.

### Libro del broker {#broker-book}

Sincroniza una [cuenta de broker](/es/config/brokers) y el gráfico dibuja lo que realmente tienes: una línea de precio por posición abierta a su coste medio, una por orden activa en su límite o stop. Los niveles aterrizan en el gráfico cuyo ticker coincide, sin contar la puntuación, así que un espacio de trabajo de varios instrumentos se anota solo. Solo lectura, y se relee únicamente cuando pulsas *Sincronizar*: una posición sin coste medio no recibe línea y se cuenta como tal en lugar de colocarse en un sitio plausible.

### Backtest rápido

Un bloc de notas para operar un gráfico a mano: **haz clic en el gráfico para abrir una posición, otro clic para cerrarla**. Con una pulsación larga eliges el lado y el tamaño, y un clic en la flecha de un marcador lo invierte. El pyramiding, los cierres parciales y las inversiones se derivan de los fills que colocas.

La sesión abarca **todo el espacio de trabajo, no un gráfico**: cada gráfico en pantalla publica sus fills en ella y las cifras son su suma, como se lee realmente un libro de varios instrumentos. El selector del panel nombra el **gráfico destino**, aquel en el que un clic coloca una operación y el que edita el cuadro de tamaño; es el panel activo, así que elegir aquí y hacer clic allí son el mismo acto.

La franja bajo el espacio de trabajo es una línea de cifras de la sesión cuando está contraída. Expandida se puede redimensionar por su borde superior y tiene tres pestañas:

- **Operaciones**: la lista de operaciones de la sesión con entrada, salida, P&L y R (medido contra la peor pérdida abierta de la operación, ya que no hay stop que citar), más un restablecimiento.
- **Estadísticas**: win rate, expectancy, profit factor, resultados por lado, rachas y una distribución de rentabilidades.
- **Rendimiento**: la curva de P&L de la sesión, con cada operación llevando su run-up y su peor pérdida abierta, de modo que una ganadora que pasó el día en negativo se lee como tal.

El tamaño se introduce en **unidades**, **contratos** (multiplicados por un valor del punto) o **nocional**, y se convierte al precio del fill. Estos fills viven **en tu navegador, por instrumento**: es un bloc de notas para leer un gráfico, nunca datos del journal, y nada se publica en el [Trading Journal](/es/modules/journal).

El botón **Backtest** entrega el mismo instrumento al módulo [Backtest](#backtest), guardándolo antes si no estaba almacenado.

### El gráfico recuerda dónde lo dejaste

El tipo de gráfico, los indicadores y los dibujos pertenecen al **instrumento**, no a un dataset ni a un panel: se guardan en el servidor bajo las coordenadas propias del símbolo poco después de cada edición, y vuelven igual en cualquier panel, en cualquier espacio de trabajo, desde cualquier navegador. Eso vale para un símbolo que solo miraste una vez y nunca descargaste, que es el objetivo: graficar algo que no has decidido conservar ya no significa perder lo que dibujaste en él.

Lo que *no* se almacena en el servidor es la sesión del backtest rápido, que se queda en este navegador.

El interruptor de **autoguardar datos** en los ajustes del gráfico es una decisión aparte, sobre las barras y no sobre el diseño: almacena un instrumento la primera vez que lo grafica, lo que encola una descarga. Está desactivado por defecto.

### Cuando faltan datos

Una ventana que vuelve corta siempre dice **por qué**, en un aviso sobre el gráfico, conservando las barras que sí llegaron:

| Motivo | Qué ocurrió |
|---|---|
| **auth** | Falta la credencial del connector o fue rechazada. |
| **quota** | El connector alcanzó su propio [límite de peticiones](/es/config/connectors#request-limits). |
| **rate_limit** | El proveedor limitó la petición. |
| **symbol** | El proveedor no conoce este ticker. |
| **depth** | El proveedor no sirve historial tan antiguo en este timeframe. |
| **provider** | Cualquier otra cosa que devolvió el proveedor. |

## Backtest {#backtest}

*Combina señales de indicadores, dimensiona con pyramiding, mide el edge.* Elige un dataset o un portfolio entero, define reglas, ejecuta. Sin código.

### Estrategia

- **Reglas de entrada / salida** por lado, construidas a partir de comparaciones entre indicadores, precio y valores fijos. Agrupa reglas con **AND** (todas deben cumplirse) u **OR** (basta una).
- **Dirección**: largo, corto o ambos. Opciones: derivar el lado corto como espejo del largo (operadores inversos, niveles de osciladores reflejados: RSI por debajo de 30 pasa a RSI por encima de 70; los filtros de ADX, ATR y volumen se mantienen tal cual), y **stop & reverse** (invertir la posición cuando se dispara la señal contraria).
- **Stop-loss / take-profit** por lado: cada uno es una casilla que activas o desactivas de forma independiente (porcentaje de la entrada media, o un múltiplo del ATR de la última vela cerrada antes de la entrada). Sin reglas de salida, las salidas se producen mediante SL/TP o inversión.

### Sizing, cuenta y costes

- Dimensiona por **porcentaje de equity** o **cantidad/lotes/contratos fijos**, con **apalancamiento** y **capital inicial**. La cantidad fija **escala con el apalancamiento**, siguiendo la convención retail, así que un apalancamiento de 3 sobre un tamaño fijo de 1 abre 3 unidades.
- **Pyramiding**: permite hasta N entradas apiladas cuando la señal de entrada se vuelve a disparar; SL/TP siguen entonces el precio medio de entrada. Una adición se envía en la apertura, antes de probar la vela: una vela que luego toca el stop cierra la posición que hizo la adición, y un stop que la adición movió (breakeven) se prueba en esa misma vela.
- **Costes**: comisión (fija o % del nocional, por operación o por unidad) y **spread %**, para que los resultados no sean fantasía. Una comisión puede ser negativa, para un rebate de maker o un broker que paga por fill. La comisión de entrada sale del efectivo en el fill, como la adeuda un broker, así que el equity, el drawdown y las adiciones de una posición abierta son netos de ella.
- Los ajustes sin sentido (sin capital, un tamaño o apalancamiento de cero o menos, un contrato que no vale nada, una cuadrícula que no se puede operar) se rechazan antes de la ejecución, y también un dataset con una vela que no lo es (un máximo por debajo del mínimo, un precio que no es un número), nombrando esa vela.

### Sizing (avanzado)

Además de porcentaje de equity y cantidad fija:

- **Riesgo por operación**: dimensiona para que un stop-loss tocado cueste un % fijo del equity (necesita un stop en el lado operado).
- **Kelly fraccional**: dimensiona a partir del win rate y el payoff de las últimas *N* operaciones de señal de la estrategia, omitidas incluidas (una operación en breakeven no es ni ganancia ni pérdida), escalado por la fracción que elijas y con tope; se usa un tamaño de calentamiento hasta que se llena la ventana. Una racha perdedora pausa las entradas sin detenerlas para siempre: se reanudan cuando vuelve el edge.
- **Tramos de equity**: una tabla de umbrales; el tramo más alto cuyo nivel es ≤ el equity actual fija el tamaño.

### Portfolio (multiactivo)

Añade varios datasets y ejecuta una estrategia sobre todos ellos con un **reloj fusionado** (todos fijados al mismo timeframe):

- Una **vista previa de alineación** muestra la longitud del reloj fusionado, la ventana de solapamiento, las barras de calentamiento de indicadores (incluido el retroceso acumulado de un indicador personalizado encadenado) y las barras que faltan por activo, todo antes de simular.
- **Límites del portfolio**: limita el número de posiciones abiertas y la exposición total / por activo. Una posición mantenida en una apertura conserva su plaza allí aunque se cierre más tarde en esa vela.
- **Sesiones**: los activos cuyas velas abren a horas distintas (un día crypto a las 00:00 UTC, uno de Nueva York a las 13:30) actúan en ese orden. Una orden en la apertura más temprana dimensiona y comprueba sus límites sobre el cierre anterior del activo posterior, no sobre una apertura que aún no ha ocurrido.
- Un **desglose por activo** informa de operaciones, PnL neto, comisiones, win rate y exposición de cada instrumento.

### Estrategia de cuadrícula

Una escalera de niveles de precio entre un límite inferior y uno superior; cada celda compra bajo y vende en el siguiente nivel superior, **largo**, **corto** o **neutral**. Dimensiona una cantidad fija por nivel o reparte un presupuesto total entre las celdas, con stops opcionales por encima/debajo de la escalera. Los resultados informan de fills, round trips e inventario final.

- **Neutral** opera ambos lados de la línea central: las celdas por debajo compran y venden un nivel más arriba, las celdas por encima venden en corto y recompran un nivel más abajo. Necesita un número impar de niveles.
- Una compra solo reposa en una línea **por debajo** del último cierre (una venta por encima), y se ejecuta en la línea, o en la apertura cuando la vela abre más allá de ella. Los objetivos se ejecutan igual.
- Un **stop** por debajo o por encima de la escalera se ejecuta en su nivel (en la apertura si hay gap), después de los fills que el precio encontró de camino, y luego detiene la cuadrícula.
- Sin límites, la escalera abarca el rango conocido hasta el momento: el mínimo más bajo y el máximo más alto de las velas anteriores a cada una.
- Fuera de la ventana de trading con *cerrar*, el inventario sale en la apertura, antes que cualquier otra cosa en la vela.

### DCA (plan de ahorro)

Un tercer modo junto a las reglas de señal y la cuadrícula, para la forma en que se invierte realmente la mayor parte del dinero: una **cesta ponderada**, comprada con el tiempo, nunca reequilibrada.

- **Pesos, fijos.** Cada euro desplegado se reparte según los pesos que fijes por ticker. No se reequilibra nada, así que una regla que se dispara en un activo de cinco despliega la parte propia de ese activo.
- **Dinero entrante.** El capital inicial se compra de una vez en la primera barra de cada activo. Todo lo posterior es **dinero nuevo**: una aportación recurrente (por barra, día, semana, mes, trimestre o año, invertida al llegar o mantenida como efectivo), y reglas de compra que aportan un importe cuando se cumple su condición.
- **Reglas de compra**: un importe fijo, un % del efectivo, del portfolio o de la base de coste, con un número máximo de disparos y un cooldown, ejecutadas en la apertura de la siguiente barra.
- **Reglas de venta**: un % de la posición, la posición entera, un número de unidades o un importe, activadas por un **objetivo de ganancia**, una condición, o ambos, con lo obtenido mantenido como efectivo o retirado.
- Las **condiciones** son los grupos de reglas habituales del motor más dos familias escritas para este modo: **métricas de mercado** (caída desde el máximo, subida desde el mínimo, variación en N barras, variación desde el inicio) y la **posición viva** (P&L %, desviación desde la última compra, coste medio, unidades, valor, peso %, efectivo %, drawdown). Se evalúan sobre un **índice de cesta ponderada** por defecto, o por activo, lo que entonces compra solo los activos que cumplen.
- **Medidas para un plan de ahorro**, no para una estrategia: drawdown y Sharpe sobre la curva **ajustada por aportaciones (ponderada por tiempo)**, para que un ingreso no se lea como un rally; rentabilidad sobre el dinero aportado; **IRR** para la rentabilidad ponderada por dinero; y un benchmark del mismo total aportado desplegado de una sola vez al inicio.

El sizing, el pyramiding y los stops no se aplican aquí: las reglas del propio plan deciden cada fill.

### Ventana de trading

Un paso de **Filtros** decide *cuándo* puede abrir la estrategia, en el reloj que elijas: una zona horaria con nombre (`America/New_York`), que sigue el horario de verano, o un offset UTC fijo, que no:

- **Días de la semana** y **sesiones** (varias por día, un final anterior a su inicio cruza la medianoche).
- **Calendario**: opera solo en fechas dadas, o nunca en ellas. *Nunca* prevalece sobre *solo*.
- Fuera de la ventana, la posición se **mantiene** o se **cierra**, y las adiciones de pyramiding también pueden bloquearse. Se filtran las entradas; las salidas, stops y take-profits siguen ejecutándose en cada barra.

### Costes y realismo de ejecución

- **Slippage**: un número fijo de ticks o un porcentaje del precio, aplicado a cada fill.
- **Funding**: una tasa anual constante sobre el nocional abierto para estimaciones de perpetuos (los largos pagan, los cortos cobran).
- **Circuit breakers**: detener el trading tras una pérdida diaria máxima (para el día) o un drawdown máximo (para la ejecución).
- **Perfil del instrumento**: tick de precio, paso de lote, cantidad mínima y multiplicador de contrato, para que tamaños y precios se ajusten a un contrato realista. Cada fill y cada stop, objetivo, límite y línea de cuadrícula se redondea al tick, en contra del trader: una compra paga el tick superior, el stop de un largo queda un tick más abajo.

### Ejecución de órdenes {#execution}

Cómo se ejecutan siempre los stops y objetivos:

- Una operación se prueba contra su stop y take profit **en la vela en la que se abre**, no desde la siguiente.
- El **take profit es una orden límite**: se ejecuta en el objetivo, sin slippage ni spread cobrado, cuando el lado que opera lo alcanza (el bid para un largo, el ask para un corto), o solo cuando el precio **atraviesa** el objetivo si eliges eso (Avanzado, *Ejecución*). Una vela que **abre más allá del objetivo** lo ejecuta en esa apertura, antes que cualquier otra cosa en la vela.
- El **stop loss es una orden stop**: se ejecuta en el stop, o en la apertura cuando la vela hace gap más allá, y paga spread y slippage.
- Cuando una vela alcanza **tanto** el stop como el objetivo y nada más dice cuál fue primero, gana el stop.
- Una operación cerrada dentro de una vela (stop, objetivo, límite) no se reabre en la apertura de esa vela, un precio anterior a la salida: una nueva entrada espera a la siguiente vela.
- Toda señal decidida en un cierre se ejecuta en la **siguiente apertura**: una entrada, la condición de salida y *salir cuando la entrada deja de cumplirse*. Nada se ejecuta en el cierre que produjo su propia decisión.
- MAE y MFE cuentan solo lo que la operación vivió: desde su fill hasta su salida, nunca el resto de la vela después de que saliera.

Opciones en el paso **Avanzado** (*Ejecución*), todas desactivadas por defecto:

- **Orden de entrada**: mercado, o **límite**. Un límite reposa por debajo de la referencia para comprar y por encima para vender, a un offset (porcentaje, distancia de precio o múltiplo de ATR) del cierre de la vela de señal o de la apertura de la siguiente vela. Sigue siendo válida durante el número de velas que fijes, se ejecuta al tocar o solo cuando el precio la atraviesa, y se ejecuta a su propio precio sin slippage (en la apertura cuando una vela abre más allá). Una señal que sigue cumpliéndose no mueve una orden en reposo. Las adiciones de pyramiding siguen la misma regla.
- **Orden de salida**: la misma elección para las salidas por señal (la condición de salida y *salir cuando la entrada deja de cumplirse*). Un límite de salida que no se ejecuta a tiempo **pasa a mercado** en la siguiente apertura o se **cancela**. Un stop-and-reverse sigue siendo una inversión a mercado.
- **Comprobar timeframe inferior para SL/TP**: cuando una vela alcanza tanto el stop como el objetivo, o un límite se ejecuta a mitad de vela, la ejecución lee un timeframe inferior del mismo instrumento (mismo proveedor) dentro de *esa vela solamente* para ver qué fue primero. Lee primero un dataset almacenado en ese timeframe, luego velas descargadas por una ejecución anterior, que se conservan para ejecuciones posteriores. *Auto* toma el timeframe almacenado más fino, y en su defecto el más fino que el proveedor sirve en una petición por vela. Las velas de timeframe inferior que no coinciden con la vela (extremos distintos) no se usan.
- **Descargar velas de timeframe inferior que faltan**: con esta opción, las velas que una ejecución necesita y no tiene se descargan del proveedor, y solo esas. Tras una ejecución, un aviso indica cuántas velas se resolvieron como stop por falta de ellas, con el coste (peticiones y tiempo) y una casilla para obtenerlas. Una descarga de hasta unos 4 minutos empieza sola en una ventana de progreso; una más larga te espera, con el límite de tasa del proveedor y la cuota restante en su connector, ya que puede fallar cuando no queda suficiente. La ejecución se repite entonces con las velas descargadas. Cuando el proveedor se niega (sin permiso o suscripción para esos datos, una clave, un límite de tasa) o falla tres veces seguidas, la ejecución deja de pedirle y lo dice junto a los resultados. Una sesión paper con la opción descarga las velas bajo la vela que acaba de cerrar cuando las necesita, esperando un momento a que el proveedor las publique; sin la opción, o cuando nunca llegan, gana el stop.
- **Usar precios bid/ask**: los fills leen el bid y el ask en lugar del punto medio y el spread (una compra a mercado paga el ask, el stop y el objetivo de un largo se activan con el bid). Se obtienen del proveedor solo para las velas donde puede producirse un fill, de la forma más sencilla que ofrece (ver la tabla de abajo), y se conservan para ejecuciones posteriores. Una obtención corta empieza sola tras la ejecución, una larga pregunta antes, con la misma ventana de progreso que el timeframe inferior. Una vela sin bid/ask usa el spread, y el resultado dice cuántas lo hicieron. Los proveedores sin bid/ask histórico dejan la opción desactivada y dicen por qué.
- **Comisión maker aparte** (en el bloque Costes): los fills límite (límites de entrada y salida, take profit) pagan su propia comisión, negativa para un rebate.
- **Precios en bruto**: las acciones y los ETF se valoran con barras ajustadas por splits y dividendos cuando el proveedor las da (Yahoo, EODHD, Alpaca; las barras de Interactive Brokers vienen ajustadas por splits). Marca esto para ejecutar con los precios tal como se negociaron. Un dataset de acciones sin serie ajustada se marca junto a los resultados.

El resultado muestra las órdenes límite colocadas, ejecutadas y expiradas, cuántas velas tenían a la vez un stop y un objetivo al alcance y cómo se resolvieron (timeframe inferior o peor caso), y cuántos fills se valoraron con bid/ask.

#### Bid/ask y precio en vivo por proveedor {#bid-ask-providers}

| Proveedor | Bid/ask histórico (backtest) | Precio en vivo (stops, objetivos, límites en paper) | Bid/ask en vivo (fills en paper) |
|---|---|---|---|
| Capital.com | Velas bid y ask | Sí | Sí |
| OANDA | Velas bid y ask | No, se comprueba al cierre de la vela | No se usa |
| Interactive Brokers | Series de velas bid y ask | Sí | Sí |
| FOREX.com | Velas bid y ask | No, se comprueba al cierre de la vela | No se usa |
| Alpaca | Cotizaciones en la apertura y el cierre de la vela (acciones, ETF, crypto) | Sí | Sí |
| Massive | Cotizaciones en la apertura y el cierre de la vela (acciones, ETF, opciones, forex) | Sí | Sí |
| Binance, Binance Futures, Kraken, OKX, Bitget, Coinbase | Ninguno, se aplica el spread | Sí | Sí |
| TradeStation, Yahoo, EODHD, Alpha Vantage | Ninguno, se aplica el spread | No, se comprueba al cierre de la vela | No se usa |

Con cotizaciones leídas en la apertura y el cierre, el spread dentro de la vela es su media alrededor del propio máximo y mínimo de la vela.

#### Paper trading sobre el precio en vivo {#paper-live}

Una sesión paper lee sus señales en velas cerradas, en su timeframe, y vigila sus niveles sobre el precio en vivo entretanto:

- El **stop loss, el take profit y las órdenes límite** (de entrada y salida) se activan con el primer precio en vivo que los alcanza, sin esperar al cierre de la vela. El **trailing stop** sigue moviéndose en cada cierre, y el precio en vivo lo activa en el nivel fijado entonces.
- El fill se valora con el bid/ask en vivo cuando la estrategia usa bid/ask, y en caso contrario con el precio en vivo con el spread y el slippage de los ajustes. Se **avisa al instante**, y la siguiente ejecución lo reproduce en su vela tal como ocurrió, sin una segunda alerta.
- Un proveedor sin precio en vivo para el instrumento se nombra en el diálogo de la sesión: ahí, los stops, objetivos y límites se comprueban al cierre de la vela. Si el feed en vivo se detiene, o la app está sin conexión, la vela vuelve a tomar el control y el registro de la sesión lo dice.
- Una entrada se **avisa en el cierre que da su señal**. Una orden a mercado se anuncia al precio de ese cierre, con su tamaño (tomado sobre el equity valorado en ese cierre) y su **importe** de dinero para un broker que acepta una orden nocional en unidades fraccionarias, y luego se ejecuta en la siguiente apertura: con el primer precio en vivo cuando el proveedor emite uno (su stop y objetivo se vigilan entonces en vivo desde ese fill), y en caso contrario en la apertura de esa vela una vez cerrada. El precio se actualiza sin una segunda alerta. Una orden límite se anuncia cuando se coloca (*Triggering limit order*), y su fill se avisa cuando ocurre.
- Una entrada límite ejecutada en vivo recibe su stop y objetivo en la siguiente ejecución; hasta entonces esa posición está protegida al cierre de la vela.

El paper trading ejecuta el mismo motor que el backtest, con las mismas opciones: con *Descargar velas de timeframe inferior que faltan* o bid/ask activados, una ejecución obtiene lo que necesita la vela que acaba de cerrar, esperando un momento a que el proveedor la publique. La fila de la sesión lista las órdenes límite que tiene activas.

### Estrategias e indicadores personalizados {#strategies-and-custom-indicators}

- **Estrategias con nombre**: guarda, busca, duplica y edita configuraciones de estrategia completas.
- **Versiones de estrategia** (opcional): con el versionado activado en [Ajustes → Versiones](/es/config/settings#versioning), una estrategia guardada puede versionarse desde el menú de historial de la cabecera. Guarda una versión (con fecha y una nota opcional), revisa lo que decía cada paso, restáurala (el estado restaurado se guarda como una versión nueva, anotada con la fecha de la versión restaurada, y conserva su nombre) o elimínala. El historial se abre en una ventana con una búsqueda sobre notas y fechas; la versión que coincide con la estrategia actual está marcada. Las notas están limitadas a 500 caracteres. Las ediciones sin guardar se guardan antes de tomar la versión. Desactivar el versionado de una estrategia pregunta si conservar o eliminar sus versiones. Eliminar una estrategia con versiones pregunta si conservarlas; las conservadas pueden restaurarla desde **Estrategias eliminadas con versiones** en la pestaña Estrategias. Los agents con acceso de escritura a Backtest pueden hacer lo mismo mediante [MCP](/es/config/ai-agents), guardando una versión tras cada actualización como un commit.
- **Indicadores personalizados**: construye los tuyos a partir de pasos con nombre, sin código. Cada paso aplica un indicador integrado a una **fuente** (un campo de precio o la salida de un paso anterior) o calcula una **fórmula** que referencia pasos anteriores por nombre (`@volume / SMA(@volume)`, con `+ − × ÷`, `min`, `max`, `abs`, `clamp`). Esto te permite encadenar indicadores: una Hull MA de un RSI, un MACD de un RSI, un ratio de volumen suavizado, etcétera. Los indicadores que leen velas completas (ATR, Estocástico, ADX, VWAP…) solo se aplican al precio, no a un paso derivado. Los pasos resaltados son la salida. Los indicadores personalizados se convierten en operandos del editor de reglas junto a los integrados, y la biblioteca está **compartida con el gráfico**, que dibuja la misma definición ([Indicadores personalizados en el gráfico](#custom-indicators-on-the-chart)).
- **Selector de indicadores con búsqueda**: elige indicadores de una lista agrupada y filtrable al escribir (tanto en el editor de reglas como en el constructor de indicadores personalizados) en lugar de desplazarte por un desplegable larguísimo.

### Ventanas de fechas y barridos de parámetros (API)

Dos capacidades viven en la API y no en el formulario. Existen para el [asistente](/es/modules/agent) y para quien maneje la app mediante [MCP](/es/config/ai-agents):

- **Ejecuciones con ventana de fechas.** `from` / `to` en una ejecución (y en la vista previa de alineación) restringen el tramo simulado, que es lo que necesitan la validación walk-forward y el corte por regímenes: ejecuta 2019–2021, luego 2022–2024, y compara. `to` incluye el día entero.
- **`POST /api/backtest/sweep`**: ejecuta una cuadrícula de parámetros en el servidor y recibe **cada prueba de vuelta**, junto con el recuento de pruebas. Las rutas de la cuadrícula llegan dentro de arrays (`long.entry.conditions.0.left.period`), así que los periodos de los indicadores son barribles; limitado a 4 ejes y 64 pruebas.

Un barrido también devuelve un **Sharpe deflactado**: el Sharpe que se esperaría que alcanzara la mejor de N estrategias *sin valor*, dada la variación de estas pruebas concretas. Compara al ganador con esa vara, no con cero: en barras diarias reales, la mejor de ocho cruces de medias móviles con 0,59 frente a una vara de selección de 0,70 significa *ninguna evidencia de edge*, algo que el máximo por sí solo habría ocultado.

También devuelve la **probabilidad de overfitting del backtest** (PBO) de la cuadrícula: las curvas de equity de las pruebas se cortan en 16 bloques, cada mitad se usa una vez como conjunto in-sample y una vez como out-of-sample, y la PBO es el porcentaje de divisiones en las que el ganador in-sample queda en la mitad inferior fuera de muestra. La pestaña Comparar de [Herramientas Cuant](#quant) calcula la misma cifra sobre ejecuciones guardadas.

### Optimizer

Toma una ejecución terminada y **varía sus parámetros**: longitudes y umbrales de indicadores por lado, stops, sizing, costes, límites de portfolio, ajustes de cuadrícula y qué días de la semana excluir (se prueba cada subconjunto). Cada parámetro recibe un desde / hasta / paso, y la cabecera cuenta las variantes a medida que los amplías, con tope para que la cuadrícula siga siendo finita.

- **Antes de empezar**, estima el coste a partir de lo que midieron ejecuciones pasadas en tu máquina: milisegundos por variante, workers, tiempo total. Puedes detenerte en cualquier punto y conservar lo calculado.
- **Ranking** por la métrica que elijas (Sharpe, Sortino, rentabilidad, profit factor, win rate, expectancy, drawdown máximo, operaciones); cualquier columna reordena después. Con una división out-of-sample, cada cifra clasificada es la **in-sample**, y la rentabilidad out-of-sample se muestra pero nunca se clasifica: un ganador elegido con ella la habría visto.
- **Análisis** muestra la dispersión de la métrica elegida entre todas las variantes: peor, media, mejor y cuántas salieron positivas. Un solo número bueno significa poco si sus vecinos son terribles.
- **Recorte por pruebas múltiples**: medido sobre las velas por año que realmente tienen los datos (un mercado 24/7 tiene unas cinco veces más velas horarias que una acción), el mejor Sharpe se muestra frente a la **vara de selección**, el Sharpe que se esperaría que alcanzara la mejor de tantas estrategias *sin valor*. Por debajo de la vara, probar tantas variantes basta para explicar al ganador.
- Haz clic en una variante para leer su **backtest completo**, reproducido con sus propios ajustes. No se almacena nada hasta que la **conservas** en el historial.

### División out-of-sample

Divide los datos en una cabeza **in-sample** y una cola **out-of-sample**; la estrategia se ejecuta en ambas y los dos bloques de estadísticas (rentabilidad, profit factor, win rate, drawdown máximo, operaciones) se muestran lado a lado. Una gran diferencia entre las columnas es señal de overfitting.

### Paper trading

*Una ejecución terminada, dejada correr hacia delante.* Pulsa **Paper trade** en un resultado y la estrategia sigue operando en paper, con una programación, avisando a los canales que elijas. No hay un segundo motor: cada ejecución vuelve a simular la ventana con el backtest normal e informa de lo que cambió, así que un fill en paper es por construcción el fill que habría mostrado el backtest para las mismas velas.

- **La estrategia se congela** tal como se ejecutó, instrumentos incluidos. Editar esa estrategia después no cambia una sesión en marcha; el formulario propio de la sesión ofrece actualizarla o iniciar una copia cuando vuelves a guardar la estrategia.
- **La primera ejecución siembra el libro.** Cada round trip ya presente en la ventana se registra de una vez, resumido en un solo evento: abrir una sesión no dispara una ráfaga de alertas sobre el historial. Solo lo que ocurre después es un fill digno de un mensaje.
- **Ventana**: cuánto historial alimenta cada ejecución al motor (velas finales, o un inicio fijado).
- La pestaña **Paper** lista las sesiones con su estado, próxima ejecución, posiciones abiertas y eventos sin leer, y cada una puede ejecutarse ahora, pausarse, reanudarse o eliminarse. El registro de eventos se conserva se haya enviado algo o no, así que lo que no se pudo entregar sigue ahí cuando vuelves.

#### Programación

Cada ejecución, y sus datos, van en el reloj propio de la vela.

- **Intervalo** (cada N minutos), **diaria**, **semanal**, **mensual** o **una vez**, en una zona horaria real.
- Un intervalo se dispara en la **cuadrícula de velas**, nunca en el segundo en que se creó la sesión: cada minuto en :00, cada 15 minutos en :00 / :15 / :30 / :45, cada hora en punto.
- La ejecución **descarga la vela que espera** en los datasets propios de la sesión. La vela en curso nunca se almacena: lo que se lee es la última *cerrada*, una semana de su lunes al siguiente, y un día solo cuando ha pasado un día completo desde su marca (una vela diaria de EE. UU. llega después de las 04:00 UTC). Una última vela almacenada antes de que terminara su periodo se vuelve a leer. Una vela que el proveedor aún no ha publicado se vuelve a pedir durante un par de segundos, y un periodo sin ninguna operación no escribe nada, lo que simplemente significa que la siguiente ejecución no tiene nada nuevo que simular.
- Una ejecución que no encuentra vela nueva no cuesta nada: no simula en absoluto.

#### Qué se envía y adónde

- **Notificar**: en cada ejecución, solo en un fill nuevo, o nunca. *Cada ejecución* también informa de las tranquilas, que es la única forma de distinguir "no pasó nada" de "el motor dejó de ejecutarse".
- **Agrupar mensajes**: envío inmediato, o un resumen cada hora, día o semana. Un envío agrupado es un mensaje sobre un periodo, no un aviso por fill.
- **Avisar en** entradas, salidas o ambas, y opcionalmente solo para los **instrumentos** que nombres.
- **Canales**: los [canales de notificación](/es/config/settings#notifications) concedidos a Backtest, todos o los que elijas. Sin ningún canal configurado no se envía nada, y cada evento se registra igualmente en la app.

#### Mensajes personalizados

Tres mensajes, tres redacciones, cada una con su propio vocabulario: **Entrada**, **Salida** y **Resumen** (el agrupado). Deja un campo vacío y se usa la redacción integrada.

- Cada uno tiene un **Título** y un **Cuerpo**, escritos con marcadores:

```
{{trade.ticker}} {{trade.direction}} at {{trade.entry_price}}
```

- **Variables** lista exactamente lo que ese mensaje puede leer, con un valor de ejemplo; haz clic en una para insertarla. Una entrada lee la mitad de entrada de la operación, una salida la operación entera (P&L incluido), y el resumen lee el periodo, `since.*` (desde la última alerta), `total.*` (desde que empezó la sesión) y `open.*` (lo que se mantiene ahora mismo), además de `session.*`, `event.*` y `stats.*`. Una ruta que un mensaje no puede leer no se le ofrece.
- Los **filtros** se escriben `| name:arg`, y `upper`, `lower`, `trim` y `json` no llevan ninguno:

```
{{trade.pnl | round:2}} {{since.from | date:YYYY-MM-DD}} {{trade.exit_reason | default:-}}
```

- La **Vista previa** es el propio renderizador del servidor, así que lo que muestra es lo que se enviaría. Se ejecuta sobre una operación de ejemplo y las últimas cifras de la sesión, de modo que un valor que la sesión aún no ha producido se marca en su sitio, entre corchetes, y el resto del mensaje se sigue renderizando.

### Resultados

- Estadísticas principales: rentabilidad (frente a **buy & hold**), PnL neto y comisiones, win rate, profit factor, expectancy, drawdown máximo, Sharpe/Sortino.
- **Curva de equity** superpuesta al precio con marcadores de entrada/salida.
- Un **resumen de rendimiento** completo (beneficio/pérdida bruto, payoff ratio, mayor ganancia/pérdida, máximo de ganancias/pérdidas consecutivas, barras medias en operación…) y la **lista completa de operaciones** con **MAE/MFE** por operación (peor pérdida abierta / mejor beneficio abierto mientras estuvo en la operación), filtrable, y los motivos de salida (señal desvanecida, señal de salida, stop-loss, take-profit, invertida, fin de datos).
- **Guarda ejecuciones** por nombre y conserva un historial para comparar estrategias más adelante. El menú **Informes** de una ejecución terminada la exporta entera, por lado y por activo: un **PDF** con todos los gráficos, o **Markdown** solo con las cifras. Ninguno lleva la lista de operaciones; el archivo se nombra según la estrategia y el momento de la exportación.

## Herramientas Cuant {#quant}

Analítica sobre tus datasets, tus backtests guardados y, para derivados, directamente de un proveedor. Las pestañas se agrupan en seis grupos.

Qué pestañas necesitan qué:

- **Activo** y **Multiactivo** leen datasets almacenados de [Datos históricos](#histdata).
- **Estrategia** lee ejecuciones guardadas de [Backtest](#backtest).
- **Sizing** y **Calculadoras** toman números que escribes (el vol targeting también lee un dataset).
- **Derivados** lee un [connector](/es/config/connectors) de Interactive Brokers o Massive concedido a Cuant.

### Activo

Un dataset y una ventana de tiempo, compartidos por cada pestaña del grupo.

- **Riesgo**: volatilidad histórica anualizada, drawdown máximo, **Value at Risk** y **Conditional VaR** a tu confianza. La cola más allá del VaR se estima de tres maneras (normal, Cornish-Fisher, un ajuste Generalized Pareto de las peores pérdidas). Una tabla lista los peores drawdowns con su profundidad, fechas y las barras que tardó en llegar al fondo y en recuperarse, ya que una sola cifra de drawdown máximo oculta cuánto tardó en llenarse el agujero.
- **Estadísticas**: qué tipo de serie es.
  - Distribución frente a una normal de la misma media y volatilidad: asimetría, curtosis en exceso, un gráfico QQ.
  - Dependencia serial: autocorrelación de los rendimientos y de los rendimientos absolutos, con p-valores de Ljung-Box. Los rendimientos rara vez muestran alguna, los rendimientos absolutos normalmente sí: eso es el volatility clustering.
  - Tendencia o reversión a la media: Hurst (R/S y DFA), ratios de varianza por horizonte y la semivida del precio.
  - Estacionariedad: ADF y KPSS, leídos juntos.
  - Si el ratio de Sharpe se distingue de la suerte: t-stat, Sharpe probabilístico, longitud mínima del historial.
- **Volatilidad**:
  - Cinco estimadores sobre las mismas barras. Close-to-close usa solo los cierres; Parkinson, Garman-Klass, Rogers-Satchell y Yang-Zhang también leen el rango de la barra.
  - Sus trayectorias móviles.
  - Un **cono de volatilidad** que dice si la lectura de hoy es alta o baja para su horizonte.
  - Una previsión **GARCH(1,1)** con su persistencia y semivida del shock.
- **Regímenes**:
  - Un modelo oculto de Markov gaussiano divide los rendimientos en 2 a 4 estados, el más calmado primero, y sombrea el gráfico de precios por el estado más probable.
  - Rendimiento, volatilidad, tiempo pasado y permanencia típica de cada estado.
  - Las probabilidades de transición, y en qué estado es más probable que esté ahora el mercado.
- **Eventos**: elige una condición (gap, cierre grande, cruce de SMA o RSI, nuevo máximo o mínimo de N barras, racha, pico de volumen) y mira qué hizo el mercado después.
  - Rendimientos futuros a varios horizontes frente a la base incondicional sobre las mismas barras, con un p-valor por horizonte.
  - La trayectoria media alrededor del evento.
- **Estacionalidad**:
  - Un heatmap mes × día de la semana del rendimiento medio, la volatilidad, el rango de barra o el volumen, más tiras por mes, día de la semana y (solo intradía) hora.
  - Cada celda muestra su recuento de muestras y win rate. El reloj horario es **UTC**.

### Multiactivo

Dos o más datasets con el mismo timeframe.

- **Portfolio**: matriz de correlación, **frontera eficiente** (una nube de asignaciones aleatorias; haz clic en el punto de máximo Sharpe o mínima volatilidad para leer sus pesos) y **risk parity**.
- **Pares**:
  - Cointegración (Engle-Granger, y Johansen en ambas direcciones) y el spread con su ratio de cobertura, z-score y semivida.
  - Correlación y beta móviles.
  - Correlación lead-lag y causalidad de Granger, para ver si una serie se mueve primero.
- **Cesta**:
  - **PCA**: cuántas apuestas independientes contiene realmente la cesta.
  - Un **dendrograma** de correlación: quién se mueve junto.
  - Una asignación de **hierarchical risk parity**.
  - Una tabla de fuerza relativa a 1, 3, 6 y 12 meses.
  - Un stress test que mantiene una ponderación a través de cada crisis pasada que cubren los datos (2008, 2020, 2022 y otras).
- **Regresión**:
  - Los rendimientos de un activo sobre uno o más datasets de factores (un índice, bonos, oro, un sector).
  - Alfa con su t-stat, la beta de cada factor, R², tracking error e information ratio.
  - Captura al alza/a la baja y una beta móvil.

Mezclar clases de activos está bien, incluso con proveedores distintos: las barras se emparejan por el **periodo** al que pertenecen, no por el timestamp con el que las marcó el proveedor. Una vela diaria crypto abre a las 00:00 UTC y una de acciones de EE. UU. al inicio de la sesión de Nueva York, y ambas son el mismo día. De ahí se siguen dos cosas, y el panel dice cuál se aplicó:

- Una cesta que mezcla un mercado 24/7 con uno de horario bursátil se mide **semanalmente**. Alineado a diario, el movimiento del fin de semana del activo continuo caería en la misma fila que el lunes del otro y subestimaría cuánto se mueven realmente juntos.
- La anualización se **cuenta sobre el reloj** en lugar de asumirse: los mismos datasets diarios son 252 periodos al año en un exchange y 365 en un mercado 24/7.

Los datasets intradía son la excepción: las barras de 4h ancladas a una sesión de trading y las ancladas al reloj están separadas 90 minutos, así que una cesta intradía de proveedores mixtos se rechaza en lugar de aproximarse. Usa datasets diarios, o un solo proveedor para toda la cesta.

### Estrategia

Ejecuciones de backtest guardadas. Cada ejecución se reproduce en el servidor para regenerar sus operaciones exactas.

- **Monte Carlo**: remuestrea las operaciones de una ejecución miles de veces (una a una, o en bloques para mantener juntas las rachas).
  - **Bandas de percentiles** sobre la trayectoria de equity, el equity final y el drawdown máximo.
  - La probabilidad de terminar en pérdida.
  - Un **riesgo de ruina**: el porcentaje de trayectorias cuyo equity llegó alguna vez a un umbral que fijas.
  - La curva de equity real dibujada encima.
- **Operaciones**: lo que valen las operaciones por unidad de riesgo.
  - Expectancy en divisa y en **R**, la distribución de múltiplos R y **SQN** (sobre un máximo de 100 operaciones, con la graduación de Van Tharp).
  - El scatter **MAE/MFE**: cuánto calor soportaron las ganadoras, hasta dónde corrieron antes las perdedoras.
  - 1R es el stop cuando la ejecución tiene un stop porcentual, y en caso contrario la pérdida media, y la página dice cuál. Las ejecuciones de cuadrícula y DCA no registran MAE/MFE, así que el scatter se omite para ellas.
- **Comparar**: de 2 a 20 ejecuciones en sus fechas comunes.
  - Curvas de equity rebasadas, una tabla de rentabilidad, volatilidad, Sharpe y drawdown, y la correlación de sus rendimientos.
  - El **Sharpe deflactado** de la mejor ejecución, contando las demás como las pruebas entre las que fue elegida.
  - La **probabilidad de overfitting del backtest** (PBO, por validación cruzada combinatoriamente simétrica sobre 8 a 16 bloques). Una PBO superior al 50 % significa que el ganador in-sample normalmente cae en la mitad inferior fuera de muestra.

### Sizing

- **Tamaño de posición**: a partir de tu stack, entrada, stop y riesgo (porcentaje o fijo), el **tamaño, nocional, margen, exposición y reward:risk**. Puede **sugerir stops** a partir de un dataset (volatilidad, ATR, swing) y rellenar la entrada con el último cierre.
- **Kelly**: la fracción de Kelly a partir del win rate y el payoff, con medio y cuarto de Kelly. El Kelly completo maximiza el crecimiento a largo plazo pero oscila mucho; la mayoría de traders dimensionan a medio o cuarto.
- **Vol targeting**:
  - Mantén volatilidad objetivo ÷ volatilidad estimada del activo, con la estimación móvil o EWMA, limitada a un apalancamiento máximo.
  - Cada barra se dimensiona con la estimación conocida antes de ella, así que no hay look-ahead.
  - Muestra el peso y las unidades a mantener ahora para tu equity, y el track escalado frente a mantener el activo plano.
- **Riesgo de ruina**: la probabilidad de que un win rate, un payoff y un riesgo por operación alcancen un drawdown dado, con un importe fijo o una fracción fija arriesgada por operación. Tres respuestas:
  - Las formas cerradas: Vince, para un importe fijo; la cota de Cramér-Lundberg, para cualquiera de los dos dimensionamientos.
  - Una simulación sobre el número de operaciones que fijes, con su error estándar y la curva de ruina por número de operaciones.

### Calculadoras

- **Opciones**: precio y greeks de Black-Scholes-Merton (vega y rho por punto, theta por día), un árbol binomial para ejercicio americano con la prima de ejercicio anticipado, y la **volatilidad implícita** de un precio cotizado.
- **Base de futuros**: a partir de un precio spot y uno de futuros, la base, el carry que implica por año, el repo implícito, el valor justo a tu tipo y rendimiento, y el roll yield al siguiente contrato.
- **Capitalización**:
  - A dónde llevan un capital y una rentabilidad por periodo, con aportaciones.
  - La rentabilidad necesaria para alcanzar un objetivo, y cuántos periodos lleva.
  - La ganancia necesaria para remontar un drawdown.
- **Test de Sharpe**: para un Sharpe citado sin sus datos.
  - ¿Se distingue de cero, o de un benchmark?
  - ¿Qué longitud de historial necesita?
  - ¿Qué queda de él una vez contabilizado el número de estrategias probadas (Sharpe deflactado, con asimetría y curtosis)?

### Derivados

Estos leen directamente del proveedor en lugar de un dataset almacenado, así que necesitan un connector de **Interactive Brokers** o **Massive** concedido a Cuant.

Una ejecución son decenas de peticiones al proveedor, marcadas por el ritmo del proveedor. La página muestra las peticiones hechas sobre las planificadas y lo que se está obteniendo. Lo obtenido se conserva seis horas, así que cambiar un tipo o una regla de roll recalcula sin volver a preguntar al proveedor.

- **Curva de futuros**: los contratos con vencimiento de un producto (`ES@CME` en Interactive Brokers, `ES` en Massive), expirados incluidos.
  - La **estructura temporal** en la última sesión terminada.
  - El **roll yield** entre el contrato frontal y el siguiente a lo largo del tiempo.
  - Una serie continua de mantener el frontal y hacer roll, ajustada hacia atrás por ratio para que termine en el precio de hoy, junto a la empalmada que salta en cada roll.
  - El roll es una regla de calendario: el frontal es el contrato más cercano con más de *N* días restantes.
  - Con un ticker spot (por ejemplo `SPX` como índice), añade la base, el carry `ln(F/S)` por año, el repo implícito y el desajuste frente al valor justo al tipo y rendimiento que introduzcas.
  - Las curvas de Massive usan el precio de liquidación de cada sesión.
- **Superficie IV**: la cadena de opciones de un subyacente.
  - Unos cuantos vencimientos repartidos entre los días mínimo y máximo que fijes, strikes out-of-the-money a cada lado, cada precio convertido en una volatilidad implícita de Black-Scholes-Merton.
  - Sonrisas por vencimiento, la **estructura temporal ATM**, **risk reversal** y **butterfly** de 25 delta, y una comprobación de que la varianza total ATM nunca cae de un vencimiento al siguiente.
  - Interactive Brokers valora cada opción en el último punto medio horario, tomado la misma hora que el subyacente, así que no hace falta suscripción de datos de mercado de opciones. Massive valora cada una al cierre de la sesión.
  - La página te dice el número de peticiones antes de empezar: con una clave gratuita de Massive (5 por minuto) una cadena tarda varios minutos.
- **Implícita frente a realizada** (solo Interactive Brokers): la volatilidad implícita a 30 días del subyacente, hasta diez años atrás, frente a la volatilidad close-to-close antes y después de cada día.
  - La **prima de volatilidad**: IV menos la volatilidad que siguió.
  - **IV rank** y **percentil** sobre un lookback que eliges.
  - Lo bien que la IV pronosticó la volatilidad realizada (una regresión de la realizada posterior sobre la IV).
  - La correlación de los cambios de IV con los movimientos de precio.
