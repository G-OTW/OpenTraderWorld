# Portfolios y patrimonio

Módulos independientes para seguir lo que vigilas, lo que posees, lo que te cuesta y cuál podría ser la factura fiscal.

## Watchlists {#watchlists}

Listas con nombre de símbolos que quieres tener controlados: sin posiciones, sin libro mayor, solo cotizaciones.

- **Añade símbolos** buscando (crypto mediante CoinGecko, acciones/ETF mediante Yahoo), empieza desde una **plantilla curada** (Crypto Top 10, Magnificent 7, ETF de índices de EE. UU., Semiconductores), o **importa un portfolio del Seguimiento de Portfolio**, donde reimportar concilia en lugar de duplicar.
- Cada fila muestra el precio en USD en vivo, **variaciones a 24 h / 3 d / 7 d / 30 d**, un **minigráfico de 30 días**, el exchange y una **nota** libre por símbolo. Ordena por cualquier columna, filtra por nombre.
- **Autoactualización** por lista, de cada minuto a diaria (15 min por defecto). La página estima la tasa de peticiones y **avisa antes de que un intervalo arriesgue una limitación de las APIs gratuitas**. Las cotizaciones se cachean en el servidor, así que reabrir la página es instantáneo y nunca toca a los proveedores.

### Fuentes de cotización personalizadas

Las fuentes públicas (CoinGecko / Yahoo) funcionan de serie sin configuración. Si tienes tu propia cuenta de datos de mercado, enchufa un **[connector de datos](/es/config/connectors)**, la misma cuenta de proveedor compartida que usan [Datos históricos](/es/modules/market-data#histdata) y el gráfico, creada una vez y concedida a Watchlists. Cada connector lleva sus propias credenciales (escritas, o elegidas de la [Bóveda](/es/config/settings#vault)) y su propio límite de peticiones.

- Una lista puede **fijar un connector como su fuente predeterminada**, y cada símbolo puede sustituirla: *seguir la lista*, *auto* o un connector concreto.
- Los **tickers de proveedor** por símbolo (`BTCUSDT`, `AAPL.US`, …) se derivan automáticamente y siguen siendo editables cuando un proveedor nombra un símbolo de otra manera.
- Una cotización que falla aparece **en su propia fila**, así que un símbolo malo no oculta el resto de la lista.

::: warning Conoce los límites de tu plan
Una lista respaldada por una fuente personalizada desbloquea **intervalos de actualización de 5 s a 30 s**. Son lo bastante rápidos para agotar un plan de API enseguida: las llamadas en exceso fallan y pueden hacer que bloqueen tu clave. Vigila los contadores en **Ajustes → Tasa de API**.
:::

### Alertas de precio

Cualquier símbolo puede llevar alertas, definidas desde la campana de su fila. Cada una se lee como una frase que ensamblas de izquierda a derecha, *avísame cuando BTC se mueva ±5 % desde ahora*:

- **Un nivel** (precio por encima o por debajo de un valor), o **un movimiento** medido en **%** o en **$**, al alza, a la baja o en cualquier sentido.
- Un movimiento se mide **desde ahora**, o en una **ventana móvil** (1h, 4h, 12h, 1d, 3d, 7d, 30d).
- **Una sola vez o repetida**, con un retardo de rearme (de 5 m a 1 d) para que una sola oscilación no se dispare en cada actualización.
- **Destinos**: la bandeja de la app más cualquier [canal de notificación](/es/config/settings#notifications) que elijas por alerta. Watchlists solo puede apuntar a los canales que se le han concedido.

Las alertas se evalúan **en el servidor dentro del bucle de actualización**, así que se disparan con la página cerrada y el navegador apagado.

### Descripción de la lista

Una watchlist lleva una descripción editable bajo su nombre, para qué sirve realmente la lista.

### Columnas y vistas

- **Selector de columnas**: el engranaje al final de la cabecera de la tabla elige las columnas de cada lista: capitalización, volumen, PER, rentabilidad por dividendo, RSI(14), rango de 52 semanas, distancia al máximo de 52 semanas, objetivos. Un dato que la fuente no ofrece muestra **n/d**, nunca 0 (Yahoo ofrece el rango de 52 semanas y el volumen pero no capitalización, PER ni rentabilidad; CoinGecko ofrece capitalización y volumen).
- **Barra de posición de 52 semanas**: dónde está el precio en su rango anual, visible por defecto. Las cripto en la fuente por defecto no tienen rango de 52 semanas; un año de velas diarias guardadas lo completa si existe.
- **Mapa de calor**: un mosaico por símbolo, según la capitalización (mismo tamaño si se desconoce), coloreado por la variación de 24 h, 7 d o 30 d.
- **Comparar**: todos los símbolos rebasados a 100 en 1M, 3M o 1A. 1M sale de las cotizaciones en caché; 3M y 1A necesitan velas diarias guardadas en [Historical Data](/es/modules/market-data#histdata), y un símbolo sin ellas aparece como sin cobertura con la solución.
- **Correlación**: la correlación por pares de los rendimientos diarios en la ventana, en los días en que ambos símbolos cotizaron. Un par con menos de 10 días en común muestra un guion.

### Herramientas de fila

- **Secciones**: agrupa filas (núcleo, candidatos, salidas), pliega un grupo, arrastra una fila a otra sección, renombra una sección desde su cabecera.
- **Banderas de color**: un punto de color antes del símbolo y un filtro por cada bandera usada.
- **Objetivos**: un objetivo de compra y uno de venta por símbolo con la distancia a cada uno; la campana junto a un objetivo abre una alerta de nivel rellenada a ese precio.
- **Posición virtual**: «si hubiera comprado N a P el D», con el P/L y el % bajo el precio. No se registra nada en ningún sitio.
- **Eventos**: próximas fechas de resultados, ex-dividendo y split por fila y en una franja Próximos, leídas de lo que [Fundamentals](/es/modules/fundamentals) ha guardado. Sin datos guardados, la franja lo indica.
- **Insignia de noticias**: cuántas noticias de News & Research citan el símbolo desde la última vez que la lista se marcó como leída; un clic abre las noticias filtradas por él.
- **Abrir en el gráfico**: abre el símbolo en el gráfico con el ticker de su proveedor. Una cripto en la fuente CoinGecko por defecto necesita antes una fuente de cotización fijada.
- **Añadir a la cartera**: añade el símbolo a una cartera elegida de Portfolio Tracker con una primera compra rellenada al precio actual.

### Pegar tickers y resumen diario

- **Pegar**: el diálogo Añadir símbolos acepta una lista pegada (comas, espacios o saltos de línea). Cada ticker solo se resuelve con una coincidencia exacta, no se añade nada antes de tu confirmación, un ticker ambiguo lista sus coincidencias para elegir y uno desconocido dice cómo encontrarlo.
- **Resumen diario de movimientos**: desde los ajustes de la lista (engranaje junto al título), una vez al día a la hora UTC elegida, las tres mayores subidas y bajadas en 24 h se envían a los canales de notificación elegidos.

## Seguimiento de Portfolio {#portfolios}

Valor en vivo de tus tenencias reales, un portfolio por cuenta o tema.

- **Añade activos** buscando (monedas crypto o acciones/ETF) y registra **operaciones de compra/venta** (fecha, cantidad, precio, comisión, nota). El P/L realizado y no realizado, el coste medio y los pesos se calculan a partir del libro mayor.
- **Divisa de operación por activo**: cada activo declara la divisa en la que se introducen sus operaciones, para que una acción comprada en EUR no se registre como si fuera USD. Las etiquetas del formulario siguen esa divisa. Las cotizaciones spot siguen en USD: el coste base y el P/L realizado se convierten a la **fecha propia de cada operación** usando los tipos FX del [Trading Journal](/es/modules/journal), así que una compra de hace tres años conserva su tipo histórico. Un popover del formulario explica de dónde viene cada precio.
- **Autoactualización**: un paso único de conciliación comprueba cada tenencia contra su fuente de precios; corrige las que salgan *sin resolver* (o márcalas como manuales) y activa la **actualización diaria automática**, tras lo cual los precios se actualizan en segundo plano cada día.
- Por portfolio: valor, coste base, P/L no realizado/realizado/total, mejor y peor activo, **asignación** por activo o clase, y un **gráfico de valor en el tiempo** (día/semana/mes/año) que se va rellenando a medida que se acumulan actualizaciones.
- La **descripción** sigue siendo editable tras la creación, junto a una nota plegable de **tesis de inversión** guardada con el portfolio, sobre por qué tienes lo que tienes.

### Efectivo, ingresos y costes

El libro mayor no son solo compras y ventas. **Efectivo e ingresos** en la pestaña de operaciones registra un
**ingreso de fondos**, una **retirada**, un **dividendo**, un **interés**, un **cupón**, una **comisión** o un
**impuesto**, cada uno en su propia divisa y con una comisión retenida opcional. Un ingreso puede nombrar la
tenencia que lo pagó, o ninguna cuando vino de la propia cuenta.

A partir de esas filas el portfolio obtiene un saldo de efectivo por divisa, un total de ingresos y un
patrimonio neto real (posiciones más efectivo). El efectivo negativo se muestra, nunca se recorta: significa margen, o un
libro mayor al que le faltan sus ingresos de fondos, y ambas cosas merecen verse.

### Análisis

La pestaña **Análisis** responde rendimiento, riesgo y exposición en un solo lugar. Siete vistas
independientes, cada una pidiendo solo los datos que necesita, así que *Libro* se muestra al instante en una instalación nueva
mientras *Estrés* paga por velas.

Una vista que no puede responder **dice por qué y qué hacer al respecto**. Nunca muestra un cero que no
midió. Aún sin historial, un libro demasiado corto para anualizar, ningún benchmark elegido, sin velas
para él, sin objetivos definidos: cada caso es una frase y un botón, no un gráfico vacío.

| Vista | Necesita | Responde |
|---|---|---|
| **Libro** | el libro mayor, nada más | patrimonio neto, invertido frente a efectivo, no realizado, realizado, ingresos, asignación por clase |
| **Rendimiento** | historial diario | rentabilidad, IRR, anualizada, aportaciones netas, por ventana |
| **Riesgo** | historial diario | volatilidad, drawdown, Sharpe, Sortino, Calmar, mejores y peores periodos |
| **Benchmark** | historial diario y las velas del benchmark | lo que habría rendido el índice con tu volatilidad, alfa, beta, captura |
| **Ingresos y costes** | las filas de efectivo del libro mayor | ingresos cobrados, costes pagados, lastre anual, la curva sin comisiones |
| **Asignación** | una asignación objetivo | actual frente a objetivo, desviación, las operaciones que la cierran |
| **Estrés** | velas diarias por tenencia | repetición histórica y shocks de factores, con la parte cubierta |

Cada vista usa el mismo selector de ventana: 1M, 3M, 6M, YTD, 1A, 3A, 5A, Todo, o un rango
personalizado. Una ventana más larga que tu historial se informa como **no cubierta**, con los días que
realmente tiene, en lugar de hacerla pasar por tres años completos.

### Rendimiento y riesgo

**Rendimiento** informa una rentabilidad ponderada por tiempo junto a una IRR, y responden preguntas distintas.
La ponderada por tiempo es lo que hicieron las inversiones, ajustada por ingresos de fondos, porque una aportación
no es un rally. La IRR es lo que **tú** obtuviste, ponderada por dinero, así que comprar en buen momento se nota
ahí y en ningún otro sitio. Las aportaciones netas se sitúan al lado, y una rentabilidad sobre menos de una quinta parte
de un año (unos 73 días) no se anualiza: multiplicar seis semanas por ocho es una previsión, no una medida.

**Riesgo** lee la misma curva: volatilidad, drawdown máximo, Sharpe, Sortino, Calmar, el porcentaje de
días que acabaron al alza, mejor y peor día, mes, trimestre y año, y cada drawdown superior
al 2 % con **cuánto tardó en recuperarse**. Uno aún abierto se marca como en curso, con lo lejos
que estás del último máximo y cuántos días lleva.

El factor de anualización se **mide a partir de tu propia curva**, no se asume. Un libro de acciones opera
unos 252 días al año y uno crypto 365, y uno mixto no es ninguno de los dos. Sharpe y
Sortino usan la tasa libre de riesgo definida en los ajustes de medición.

### Benchmark

Elige un instrumento del que tengas velas diarias (SPY, QQQ, BTCUSDT) y la página responde la
única pregunta que zanja una discusión: **lo que habría rendido ese índice con tu
volatilidad**, junto a lo que realmente ganaste. Batir al índice asumiendo el triple de su riesgo
no es batirlo.

Debajo: rentabilidad total y anualizada de ambos, volatilidad, drawdown máximo y Sharpe lado a
lado, y luego alfa, beta, tracking error, information ratio y captura al alza y a la baja.

Tu libro se mide durante las **sesiones propias del benchmark**. Compara una cartera 24/7 con un
índice día a día y cada lunes del índice se come un fin de semana tuyo, lo que
subestima en silencio tu rentabilidad.

### Ingresos y costes

Lo que cobraste, lo que pagaste y lo que te costó pagarlo. Dividendos, intereses y
cupones por un lado; comisiones de trading y comisiones de cuenta por el otro, con el lastre anual como
porcentaje de tu patrimonio neto medio.

La curva se dibuja dos veces: tal como ocurrió, y el mismo libro con las comisiones eliminadas. Las comisiones
ya están dentro de tu coste base y de tu efectivo, así que esto es una comparación, no una resta
que pudieras hacer tú mismo.

### Asignación objetivo

Indica qué porcentaje del patrimonio neto debe tener cada cubo y cuánto puede desviarse antes de contar
como fuera de rango, en **Definir objetivos**. Una asignación tiene que sumar el 100 %, y a un cubo se le puede dar
el resto con un clic. Guardar una lista vacía desactiva la vista.

La vista muestra entonces actual frente a objetivo por cubo, la desviación, si cada uno está dentro de
su banda, y las **operaciones que cerrarían la brecha**: compra tanto de aquello, vende tanto de
esto. Todo lo que tengas sin objetivo se lista en lugar de ignorarse.

Solo informativo. Nada aquí coloca una orden, y nada reequilibra por sí solo.

### Stress testing

Dos motores, y ambos te dicen qué parte de tu libro cubre la cifra.

**Repetición histórica** aplica la trayectoria diaria realizada de 2008, 2020, 2022, el 4T de 2018 o el pico crypto de 2021
a lo que tienes hoy, usando las propias velas de los instrumentos en esas fechas. Sin
modelo, sin proxy. Un instrumento que no existía entonces no tiene trayectoria: se **nombra y
excluye**, nunca se sustituye por un índice.

**Shock de factor** mueve un instrumento real (S&P 500, Nasdaq, tipos, EUR/USD, petróleo, diferenciales
de crédito) y llega a cada tenencia mediante una sensibilidad **medida**, ajustada con sus propias
velas. Una tenencia sin velas, con un historial demasiado corto o con un ajuste sin poder explicativo
recibe **ninguna beta**: aterriza en *sin explicar* con su peso, y el titular dice
"−11,8 % sobre el 74 % del libro que se pudo medir". El efectivo tiene beta cero, que es
normalmente la única diversificación ya presente.

Un shock de tipos en puntos básicos llega a un bono mediante una duración escrita en el escenario, para que la
cifra se pueda discutir. Recesión, repunte de inflación y ensanchamiento del crédito vienen como
combinaciones editables de esos tramos.

Un panel de preparación lista lo que se puede estresar y lo que no antes de ejecutar nada, de modo que un
resultado escaso se explique de antemano y no después.

### Historial diario

Cada medida anterior excepto *Libro* necesita una curva, y las instantáneas solo empiezan el día en que activas
el job diario. Un portfolio que has llevado durante seis años se mediría, si no, desde el
martes pasado. Por eso la curva se **reconstruye a partir del libro mayor y las velas almacenadas**, día a día.

El engranaje de la pestaña Análisis abre **Configuración de medición**:

1. **Nombra el ticker de velas de cada activo** y la divisa en la que cotizan esas velas. Un ticker
   de tu libro mayor no siempre es el símbolo que sirve tu proveedor, y una acción comprada en EUR
   valorada con velas en USD se desvía por el tipo de cambio.
2. **Descarga las velas que faltan**. Se encolan como jobs normales de [Datos históricos](/es/modules/market-data#histdata)
   mediante los connectors concedidos a portfolios. Si ninguno lleva uno de tus
   instrumentos, se nombra, con lo que hay que conceder.
3. **Reconstruye la curva**. Informa de los días reconstruidos y de los días omitidos porque una tenencia
   no tenía vela ese día. Un día que no se puede valorar no se almacena, en lugar de almacenarse mal.

Editar una operación fechada en el pasado marca la curva como **obsoleta desde esa fecha** y lo dice.
Reconstruir sigue siendo decisión tuya. Actualizar un portfolio también descarga lo que falta y extiende
la curva detrás, y te avisa cuando un broker no puede servir uno de tus instrumentos.

### Importar un libro de operaciones

**Importar** en la cabecera del portfolio lee una exportación de broker, una hoja de cálculo u otro tracker (CSV, TSV, JSON, Parquet) y convierte cada fila en una operación de compra o venta. Mismo motor de detección que la [importación del journal](/es/modules/journal#import-a-trade-book): cabeceras en seis idiomas más detección de valores, convenciones de delimitador, decimal y fecha decididas por columna, columnas no identificadas dejadas sin mapear.

Tres cosas que se niega a adivinar:

- **Qué es un símbolo.** Cada símbolo del archivo debe apuntar a un activo: uno que ya tienes en este portfolio (emparejado automáticamente), un activo nuevo a crear, o *omitir*. Un símbolo sin resolver bloquea la importación, y los activos solo se crean cuando confirmas.
- **Una fila que no es ni compra ni venta ni un tipo que reconozca** se lista como error de fila en lugar de inventarla como operación. Los dividendos, ingresos de fondos, retiradas, comisiones e impuestos **sí** se reconocen, en seis idiomas, y se registran como tales. Si el archivo no indica ningún tipo, define el valor predeterminado una vez para toda la importación.
- **Un precio ausente** se deriva de importe ÷ cantidad y se marca, nunca se rellena en silencio.

No se escribe nada hasta que validas la vista previa. Cada importación es un **lote**, revertible entero (los activos creados se quedan), y deduplicada **por portfolio**, así que reimportar el mismo archivo no cambia nada.

### Importar holdings desde un broker

**Desde un broker** en la cabecera del portfolio lee el balance de una [cuenta de broker](/es/config/brokers) en lugar de un archivo: la diferencia con tu libro mayor se muestra línea por línea, y cada línea que aceptas escribe la operación que hace que el portfolio coincida. El coste base se usa donde el broker publica uno (Interactive Brokers) y se pide donde no (los exchanges crypto publican una cantidad y nada más).

### Operaciones corporativas, lotes y traspasos

**Operaciones corporativas.** El grupo **Operación corporativa** del formulario de efectivo registra un split, un contrasplit o una escisión sobre una posición. Un split multiplica las unidades y divide el coste unitario por la misma proporción; no se realiza nada. Una escisión traspasa la parte de coste que indique a la nueva posición, con las fechas de compra de la matriz, que conserva sus unidades.

**Lotes fiscales.** Haga clic en un símbolo para abrir la posición. **Lotes** lista cada compra abierta con su coste, valor, días de tenencia y plazo corto o largo. El método de casación, el umbral de tenencia y la ventana antiaplicación vienen de su primer perfil de [Tax Calculator](#taxcalc); sin perfil se usa FIFO sin plazo ni regla antiaplicación, y la pantalla lo indica.

**Traspaso entre carteras.** **Traspaso** en la posición mueve unidades a otra cartera con sus lotes, su coste y sus fechas de compra. No se realiza nada en ningún lado; el activo de destino se crea si falta.

### Ingresos, tesis y clasificación

**Calendario de dividendos.** La pestaña **Ingresos** prevé el ingreso anual por posición (unidades × importe por pago × pagos al año), muestra la rentabilidad sobre coste, las próximas fechas ex y de pago, y lo cobrado en doce meses junto a la previsión. Las fuentes de cotización no dan dividendos: usted introduce importe, frecuencia y fechas en la posición, y una posición sin ellos figura *sin estimación*. El Dashboard tiene una tarjeta **Upcoming income**.

**Tesis y fecha de revisión.** Cada posición lleva su propia tesis y una fecha de revisión. Las tesis que vencen en una semana aparecen en una franja en la página de carteras.

**Sector y país.** Escritos en la posición, o leídos de [Fundamentals](/es/modules/fundamentals) para una acción conocida. Un ETF queda sin clasificar hasta que lo escriba. El donut de asignación agrupa por sector o país, y la parte sin clasificar es su propio segmento.

### Entre carteras

**Todas las carteras.** Con más de una cartera, un bloque sobre la lista suma todo en una divisa: valor, coste, P/L, efectivo, símbolos presentes en varias carteras con su peso conjunto, divisas de negociación y mayores posiciones.

**Comparar.** Analysis → **Comparar** dibuja esta cartera y otra en una misma curva, ponderadas en el tiempo y a 100 el primer día común.

### Atribución y objetivos

**Contribución.** Bajo la tabla de rendimiento, barras muestran lo que cada posición aportó a la rentabilidad de la ventana y al mayor drawdown, a partir de cierres diarios. Las contribuciones suman la rentabilidad de la cartera reconstruida. Una posición sin cierres diarios en la ventana se nombra y las barras no se dibujan.

**Pérdidas.** Analysis → **Pérdidas** lista los lotes en negativo, del peor al menor, con la ventana antiaplicación del régimen y una marca en cada lote comprado dentro. **Enviar a Tax Calculator** lo abre con la pérdida rellenada.

**Objetivos.** Analysis → **Proyección** toma un valor objetivo, una fecha y una aportación mensual, y muestra el progreso, la aportación necesaria a la rentabilidad medida y un cono de 1000 trayectorias a partir de la rentabilidad y volatilidad medidas. Una curva de menos de un quinto de año se rechaza en vez de extrapolarse.

### Planes y notificaciones

**Operaciones previstas.** **Plan** en la posición registra una compra o venta prevista a un precio. La cotización del activo se comprueba cada hora; el plan se marca alcanzado una vez y puede avisar. **Registrarlo** abre el formulario prerrellenado.

**Alertas y resumen.** **Alertas** en la cabecera de la cartera fija el peso máximo de una posición (una posición por encima se marca), un aviso al superarlo, un aviso cuando una clase objetivo sale de su banda (también en el diálogo de objetivos) y un resumen diario o semanal con valor, variación, los tres mayores movimientos e ingresos desde el anterior. Cada alerta avisa una vez por superación y se rearma al volver a los límites. Los mensajes van a los [canales de notificación](/es/config/settings#notifications) autorizados para Portfolio Tracker.

## MyWealth {#wealth}

Patrimonio neto de **todo**: cuentas de brokerage, inmuebles, crypto, efectivo, objetos de valor. Donde el Seguimiento de Portfolio sigue tenencias con precio en vivo, MyWealth sigue cualquier activo que valoras tú mismo.

- Añade activos con un nombre, tipo, divisa y categoría, y luego **registra actualizaciones de valor** con el tiempo (precio × cantidad, o un valor directo, con una nota). El historial es editable.
- **Gráfico de patrimonio neto** por mes o año, más un desglose por categoría. Multidivisa con el mismo tratamiento FX que el journal (los activos sin tipo se excluyen y se marcan).
- **Plantillas**, como las del journal: los campos reservados de precio/cantidad alimentan el valor, los campos personalizados guardan notas por revisión.
- **Propio o adeudado**: un activo puede ser un **pasivo** (una hipoteca, un préstamo), así que el titular es un patrimonio neto real. La página muestra lo que posees y lo que debes antes de compensarlos.
- **Vincula un portfolio** en lugar de copiarlo: un portfolio vinculado se lee en vivo del tracker cada vez, así que su valor en tu patrimonio neto nunca es una copia obsoleta.
- **Antigüedad de la valoración**: indica a un activo cada cuánto debe revalorarse y la página nombra los que han superado ese plazo, el más antiguo primero. Una casa valorada hace tres años está mal en silencio, y esto es lo que rompe el silencio. Un portfolio vinculado nunca queda obsoleto: se lee, no se recuerda.
- **Calendario del préstamo**: indique importe, tipo anual, plazo y fecha de inicio (y la cuota si la del prestamista es distinta). El saldo sigue la fórmula de amortización, se escribe un valor por vencimiento y Actualizar lista las cuotas pendientes con intereses y capital.
- **Asociar un activo a su deuda**: un activo puede nombrar el pasivo que lo grava (una casa y su hipoteca); su detalle muestra el patrimonio y la relación préstamo sobre valor.
- **Deriva anual**: un tipo supuesto (+3 % para inmuebles, -15 % para un coche) estima el valor tras la última valoración real. El gráfico la dibuja punteada, nunca se guarda como valoración y la cabecera indica cuánto es estimado.
- **Cuenta de bróker y dirección de cartera**: la pestaña Cuenta de bróker del diálogo de vinculación añade el patrimonio de una cuenta como activo, leído con sus propias credenciales (la cuenta debe estar autorizada para MyWealth en Ajustes, Brókers). Un activo Dirección de cartera lee una dirección pública BTC o ETH en la cadena, valorada en USD. Ambos se leen como mucho una vez por hora y se guardan como el valor del día; una lectura fallida muestra su error en la fila, nunca un cero.
- **Objetivos**: patrimonio neto y fecha objetivo, gastos mensuales. La pestaña muestra el crecimiento medido en su curva (se necesitan 12 meses de historial), la fecha proyectada, el ahorro en 12 meses sin mercados ni deriva, la tasa de ahorro y el número FIRE (25 × gastos anuales).
- **Bajo el gráfico**: Variaciones reparte el cambio de un periodo por categoría en revalorización, altas y bajas; Antes y ahora pone la tabla de categorías de hace 1 mes, 1 año o 5 años junto a hoy; Liquidez divide las categorías marcadas como líquidas entre los gastos mensuales más la previsión de Suscripciones; Objetivos de peso fija un peso y una banda por categoría y muestra la desviación, con el cálculo de los objetivos del Portfolio Tracker.
- **Patrimonio neto real**: el botón Real deflacta la curva con un índice de precios cargado en Fundamentals (IPC de EE. UU. por defecto, otro en Ajustes). Sin índice, el gráfico lo dice y ofrece cargar el IPC de EE. UU. desde FRED.
- **Titular**: asigne cada activo a un miembro del hogar, filtre por titular y vea el reparto en la vista Titulares del gráfico.
- **Documentos**: adjunte una escritura, un contrato o un extracto (10 MB cada uno) a un activo o a una de sus valoraciones.
- **Aviso de valoraciones caducadas**: en Ajustes, un aviso semanal lista los activos cuya valoración supera su intervalo de revisión y lo envía por los canales autorizados para MyWealth.
- **Exportación y calculadora de impuestos**: el menú de la cabecera exporta activos y valoraciones en CSV; Calculadora de impuestos suma las categorías elegidas y abre la calculadora con la base del impuesto sobre el patrimonio. La demo rechaza exportaciones y descargas de documentos.

## Carteras de gestores {#mportfolios}

Explora las **carteras 13F de superinversores**: lo que tienen los gestores de fondos famosos, tamaños de posición, actividad reciente, valor reportado frente a actual, rangos de 52 semanas. Filtra por gestor o por ticker (*¿quién tiene AAPL?*).

Como los datos 13F cambian cada trimestre, puedes **guardar instantáneas** de cualquier cartera y comparar a lo largo del tiempo.

## Calculadora de Impuestos {#taxcalc}

Estimación aproximada de impuestos de trading e inversión. **No es asesoramiento fiscal.**

- Los **perfiles** parten de un **régimen fiscal** por país y ejercicio: Francia (tipo único, o profesional), Alemania, Reino Unido, Estados Unidos, Australia, Suiza, España e Italia. Un régimen fija el calendario del ejercicio fiscal, el método de coste de adquisición (precio medio ponderado, FIFO o el pool británico, modificable por perfil), la regla de recompra (wash sale) cuando el país la tiene, las exenciones y los tipos, y muestra sus fuentes y la fecha en que se revisaron sus reglas. Los **tipos personalizados** cubren cualquier otro país: tipo único o marginal, cargas sociales, exenciones de ganancias de capital y dividendos, alivio opcional por tiempo de tenencia. Cualquier perfil puede añadir **tramos de impuesto sobre el patrimonio** (p. ej. CH, ES, NO).
- **Compensación de pérdidas**: registra el resultado neto de cada ejercicio por pool (valores, cripto...). Lo que aún se arrastra al ejercicio se calcula a partir de esas filas, la pérdida más antigua primero, y caduca tras el plazo de compensación del régimen.
- Introduce cifras en modo **Resumen** (valor inicial/final, aportaciones, retiradas, parte realizada) o en modo **Detallado** (ganancias de capital, ganancias de derivados, ganancias crypto, dividendos, intereses, pérdidas previas arrastradas).
- **Cargar Trading Journal**: con el journal instalado, un clic carga el PnL realizado de un año fiscal, dividido en ganancias de capital / derivados / crypto, convertido al tipo FX de cierre de año.
- **Desde un broker**: con una [cuenta de broker](/es/config/brokers) concedida a la calculadora de impuestos, un año fiscal se lee directamente de la cuenta, se pliega en posiciones cerradas y se totaliza por línea del formulario, con cada disposición convertida al tipo de su propia fecha de salida.
- Los resultados muestran el impuesto estimado con un desglose por partida (imponible, exención, base, tipo) y el tipo efectivo. Guarda escenarios en el historial para comparar.

## Suscripciones {#subscriptions}

Todos los costes recurrentes en una lista (herramientas de trading, feeds de datos, streaming) con precio, divisa, frecuencia de facturación (semanal/mensual/trimestral/anual) y categoría.

Obtienes **gráficos de gasto** mensuales/anuales (agrupados o por suscripción), el **equivalente mensual** de cada suscripción, próximas fechas de facturación y totales del próximo mes. Pausa una suscripción para mantenerla en la lista sin contarla.

### Ciclo de vida, avisos y presupuestos

- **Historial de precios**: cambiar un precio o una divisa queda registrado. Los detalles de la fila muestran el precio en escalones, y el gráfico mensual traza una línea discontinua de «subida de costes»: lo que costaba cada mes todo lo activo, al precio de entonces.
- **Avisos de renovación**: marca «Avisarme antes del cobro», fija los días y elige canales (ninguno = todos los canales autorizados para Subscriptions). Cada fecha de cobro se avisa una vez. Un aviso necesita una fecha de inicio o de fin de la prueba.
- **Pruebas gratuitas**: marca «Prueba gratuita» y su fecha de fin. El cobro empieza ese día, la fila lleva una insignia y las pruebas a punto de convertirse salen primero.
- **Cancelar**: el botón de parada fija una fecha de fin. La suscripción sale de todos los totales desde ese día, conserva su historial y el panel cuenta lo ahorrado desde entonces. Filtro activas / canceladas / todas. Eliminar queda para los errores.
- **Pasar a anual**: para un plan cobrado más a menudo, introduce el precio anual. La columna «Ahorro anual» muestra 12 meses al ritmo actual menos el precio anual.
- **Valoración de uso**: un aviso pide valorar cada servicio activo de 1 a 5 cada trimestre. La vista «¿Merece la pena?» ordena los servicios valorados por coste mensual por punto, los más caros para su uso primero.
- **Presupuestos**: fija un tope mensual por categoría en la divisa de visualización. Una categoría por encima aparece en el panel y avisa una vez al mes.
- **Calendario**: la vista Calendario muestra las fechas e importes de cobro del mes. «Exportar .ics» descarga los próximos 12 meses como eventos de calendario (bloqueado en la demo).
- **Medio de pago y caducidad de tarjeta**: indica qué tarjeta paga, filtra por ella y ve una insignia cuando caduca en menos de 60 días o ya caducó.
- **Planes compartidos**: indica tu parte en % de un plan familiar. Precios, equivalente mensual y todos los totales usan tu parte.
- **Previsión**: la pestaña Previsión muestra los cobros de los próximos 12 meses naturales, mes a mes, con la media mensual. MyWealth lee las mismas cifras.
