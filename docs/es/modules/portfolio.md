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

Cada vista usa el mismo selector de ventana: 1M, 3M, 6M, YTD, 1A, 3A, 5A, todo, o un rango
personalizado. Una ventana más larga que tu historial se informa como **no cubierta**, con los días que
realmente tiene, en lugar de hacerla pasar por tres años completos.

### Rendimiento y riesgo

**Rendimiento** informa una rentabilidad ponderada por tiempo junto a una IRR, y responden preguntas distintas.
La ponderada por tiempo es lo que hicieron las inversiones, ajustada por ingresos de fondos, porque una aportación
no es un rally. La IRR es lo que **tú** obtuviste, ponderada por dinero, así que comprar en buen momento se nota
ahí y en ningún otro sitio. Las aportaciones netas se sitúan al lado, y una rentabilidad de menos de dos meses no
se anualiza: multiplicar seis semanas por ocho es una previsión, no una medida.

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

## MyWealth {#wealth}

Patrimonio neto de **todo**: cuentas de brokerage, inmuebles, crypto, efectivo, objetos de valor. Donde el Seguimiento de Portfolio sigue tenencias con precio en vivo, MyWealth sigue cualquier activo que valoras tú mismo.

- Añade activos con un nombre, tipo, divisa y categoría, y luego **registra actualizaciones de valor** con el tiempo (precio × cantidad, o un valor directo, con una nota). El historial es editable.
- **Gráfico de patrimonio neto** por mes o año, más un desglose por categoría. Multidivisa con el mismo tratamiento FX que el journal (los activos sin tipo se excluyen y se marcan).
- **Plantillas**, como las del journal: los campos reservados de precio/cantidad alimentan el valor, los campos personalizados guardan notas por revisión.
- **Propio o adeudado**: un activo puede ser un **pasivo** (una hipoteca, un préstamo), así que el titular es un patrimonio neto real. La página muestra lo que posees y lo que debes antes de compensarlos.
- **Vincula un portfolio** en lugar de copiarlo: un portfolio vinculado se lee en vivo del tracker cada vez, así que su valor en tu patrimonio neto nunca es una copia obsoleta.
- **Antigüedad de la valoración**: indica a un activo cada cuánto debe revalorarse y la página nombra los que han superado ese plazo, el más antiguo primero. Una casa valorada hace tres años está mal en silencio, y esto es lo que rompe el silencio. Un portfolio vinculado nunca queda obsoleto: se lee, no se recuerda.

## Carteras de gestores {#mportfolios}

Explora las **carteras 13F de superinversores**: lo que tienen los gestores de fondos famosos, tamaños de posición, actividad reciente, valor reportado frente a actual, rangos de 52 semanas. Filtra por gestor o por ticker (*¿quién tiene AAPL?*).

Como los datos 13F cambian cada trimestre, puedes **guardar instantáneas** de cualquier cartera y comparar a lo largo del tiempo.

## Calculadora de Impuestos {#taxcalc}

Estimación aproximada de impuestos de trading e inversión. **No es asesoramiento fiscal.**

- Los **perfiles** parten de **plantillas por país** (particular o profesional) y siguen siendo totalmente editables: tipo marginal de renta, cargas sociales, exenciones de ganancias de capital y dividendos, **tramos de impuesto sobre el patrimonio** opcionales (p. ej. CH, ES, NO), tramos de alivio a largo plazo.
- Introduce cifras en modo **Resumen** (valor inicial/final, aportaciones, retiradas, parte realizada) o en modo **Detallado** (ganancias de capital, ganancias de derivados, ganancias crypto, dividendos, intereses, pérdidas previas arrastradas).
- **Cargar Trading Journal**: con el journal instalado, un clic carga el PnL realizado de un año fiscal, dividido en ganancias de capital / derivados / crypto, convertido al tipo FX de cierre de año.
- **Desde un broker**: con una [cuenta de broker](/es/config/brokers) concedida a la calculadora de impuestos, un año fiscal se lee directamente de la cuenta, se pliega en posiciones cerradas y se totaliza por línea del formulario, con cada disposición convertida al tipo de su propia fecha de salida.
- Los resultados muestran el impuesto estimado con un desglose por partida (imponible, exención, base, tipo) y el tipo efectivo. Guarda escenarios en el historial para comparar.

## Suscripciones {#subscriptions}

Todos los costes recurrentes en una lista (herramientas de trading, feeds de datos, streaming) con precio, divisa, frecuencia de facturación (semanal/mensual/trimestral/anual) y categoría.

Obtienes **gráficos de gasto** mensuales/anuales (agrupados o por suscripción), el **equivalente mensual** de cada suscripción, próximas fechas de facturación y totales del próximo mes. Pausa una suscripción para mantenerla en la lista sin contarla.
