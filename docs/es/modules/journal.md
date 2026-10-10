# Trading Journal

Registra cada operación, en cualquier divisa, y obtén estadísticas de rendimiento honestas: curva de equity, win rate, expectancy, profit factor, drawdown, Sharpe y más. El journal está organizado en diez pestañas: **Desglose**, **Análisis**, **Calendario PnL**, **Operaciones**, **Estrategias y capital**, **Etiquetas**, **Plantillas**, **Comisiones y divisa**, **Importar**, **Tareas pendientes**.

## Categorías

Las operaciones viven en **categorías**: carpetas como *Scalping crypto* o *Acciones a largo plazo*, cada una con su propio color, capital y estadísticas. Créalas desde la barra de categorías; arrastra para reordenar. Eliminar una categoría elimina sus operaciones.

## Configurar el capital

En **Estrategias y capital**, da a cada categoría un **stack inicial** y registra **reposiciones** y **retiradas** a lo largo del tiempo. Es contra esto contra lo que se calculan la rentabilidad, la curva de equity y el drawdown; sin ello sigues obteniendo PnL, pero no rentabilidades.

También puedes nombrar **estrategias** con sus nombres de señal (p. ej. *Breakout, Pullback*). Etiqueta las operaciones con una estrategia/señal y la pestaña Desglose puede filtrar por ellas: así descubres qué setups realmente pagan.

## Plantillas

Las plantillas dirigen el formulario de operación. Existe una **operación estándar** predefinida; crea las tuyas por mercado o estilo:

- Los **campos reservados** (lado, precios, cantidad, comisiones, apalancamiento, multiplicador, divisa, tipo de unidad…) alimentan las estadísticas de rendimiento.
- Los **campos personalizados** (texto, números, listas de opciones…) son libres: nota del setup, condición de mercado, lo que sigas.
- Una plantilla puede definir una **tabla de comisiones predeterminada**, preseleccionada al registrar desde ella (sustituible por operación).

## Registrar operaciones

Desde la pestaña Operaciones, elige una plantilla (o la *Rápida*, que muestra todos los campos) y rellena el formulario. Dos niveles:

- **Simple**: una entrada, una salida (o deja la salida vacía para una posición abierta).
- **Avanzado**: entradas y salidas escalonadas con varios **tramos de entrada y salida** (cada uno con su precio, cantidad, comisiones, señal), más **brackets SL/TP**. Cuando un bracket se activa, márcalo y se pliega en un tramo de salida.

El formulario previsualiza la entrada media, el PnL neto y la cantidad abierta mientras escribes. Puedes adjuntar hasta dos imágenes (capturas de gráficos), elegir apalancamiento y multiplicador de contrato para derivados, y escribir tu propio feedback sobre la operación.

**El PnL se calcula al leer** y maneja posiciones parcialmente abiertas. La base de coste es conmutable entre **coste medio ponderado** (predeterminado) y **FIFO** (útil para exportación fiscal), y la elección se aplica de verdad, tanto en las estadísticas como en la vista previa del PnL en vivo del formulario.

## Comisiones

En **Comisiones y divisa**, guarda **tablas de comisiones**: fijas o porcentuales, cobradas por lote, unidad, contrato u operación (p. ej. *Acciones IBKR: 0,05 % por operación*). Seleccionar una tabla en una operación calcula la comisión automáticamente; una comisión introducida a mano siempre prevalece.

## Multidivisa y FX

Las operaciones conservan la divisa en la que las introdujiste. La **divisa del desglose** (visualización) se convierte con un feed FX diario que rellena los tipos automáticamente cada día hábil, arrastrando los tipos durante fines de semana y festivos.

Si no se puede obtener un tipo para alguna fecha, esas operaciones se **excluyen de los totales convertidos** y aparecen en **Tareas pendientes**, donde introduces a mano los tipos basados en USD que faltan (1 USD = … de esa divisa) y las operaciones vuelven a contar.

## Desglose (tus estadísticas)

Por categoría o en conjunto, filtrable por rango de fechas, ticker, lado, clase de activo, estrategia, señal y etiqueta. La barra de filtros se comparte con Análisis, así que un ámbito definido en una pantalla es el ámbito de la otra, y la lista de tickers ofrece los símbolos que el journal realmente contiene:

- **Curva de equity** en la divisa de visualización.
- PnL realizado · Rentabilidad · Win rate · Operaciones (cerradas/abiertas) · Expectancy · Profit factor · Ganancia media / Pérdida media · Mejor / Peor operación · Drawdown máximo · Sharpe y Sortino · Comisiones totales · Capital invertido · Margen desplegado · Rentabilidad sobre margen.
- **Sharpe y Sortino** se calculan sobre rentabilidades diarias frente al equity arrastrado a cada día, anualizados, con la misma definición que usa Análisis, así que las dos pantallas coinciden.

## Análisis (leer el libro)

Todo el libro en ocho pestañas, sobre los mismos filtros que el Desglose:

- **Resumen**: expectancy en **R** y R total (sobre las operaciones que llevan un stop planificado), riesgo por operación, Sharpe con Sortino a su lado, drawdown máximo con los días pasados por debajo del pico, días de trading ganados y perdidos, día medio, racha actual y mejor, y luego la **distribución de R** y el coste de tus errores etiquetados.
- **Distribuciones**: PnL neto por tiempo de tenencia, hora de entrada, día de la semana de entrada y tamaño de posición, y el recuento de operaciones por tramo de beneficio. Qué hora de tu día realmente paga.
- **Comportamiento**: lo que haces alrededor del edge, y lo que cuesta. Concentración del beneficio, tamaño tras una racha de pérdidas, ritmo tras una pérdida, cómo decae el día, y la operación tras una ganancia frente a la operación tras una pérdida. Consulta [Analítica de comportamiento](#behavior).
- **Datos de mercado**: las velas detrás de tus operaciones. MAE y MFE, eficiencia de salida, lo que se dejó sobre la mesa, distancia del stop en ATR, y resultados divididos por régimen de volatilidad y por tendencia en la entrada. Consulta [Enriquecimiento con datos de mercado](#market-data).
- **Riesgo abierto**: la única pestaña sobre el presente. Qué sigue en juego, dónde se concentra ese riesgo, y si cinco líneas abiertas son cinco apuestas o una. Consulta [Riesgo abierto](#open-risk).
- **Dispersión**: dos valores cualesquiera de la operación representados uno contra otro (fecha, número de operación, neto, PnL acumulado, rentabilidad sobre nocional, R, tiempo de tenencia, tamaño...), coloreados por resultado, lado, estrategia, ticker o clase de activo, con línea de tendencia y zoom.
- **Desglose**: rendimiento agrupado por estrategia, símbolo, etiqueta, clase de activo o lado, con operaciones, win rate, neto, expectancy, R medio y profit factor por fila.
- **Comparar**: este día, semana, mes, trimestre, año o un rango personalizado frente al inmediatamente anterior, fila a fila (neto, operaciones, win rate, expectancy, R medio, profit factor, drawdown máximo, comisiones, días de trading), sobre una tira de los últimos doce periodos.

Las operaciones cerradas sin tipo FX para su fecha se cuentan en voz alta en lugar de descartarse en silencio.

## Analítica de comportamiento {#behavior}

Las estadísticas de rendimiento dicen lo que devolvió el libro. **Comportamiento** dice cómo llegaste ahí, y cuáles de tus hábitos lo pagaron. Las mismas operaciones cerradas que el resto de Análisis, la misma barra de filtros, sin configuración adicional y sin datos de mercado: lee las operaciones que ya registraste.

Cinco tarjetas, cada una responde una pregunta.

### De dónde viene el beneficio

El porcentaje del beneficio bruto hecho por tus cinco mejores operaciones, cuántas ganadoras hacen falta para hacer la mitad, y cómo queda la cuenta sin esas cinco. A su lado, un índice de concentración: 0 significa que cada ganadora paga más o menos lo mismo, 1 significa que una sola operación paga el año. La ganancia media frente a la mediana muestra el mismo sesgo desde otro ángulo, y una curva acumulada lo dibuja.

El número que hay que mirar es el neto sin las cinco mejores. Si es negativo, el edge descansa en valores atípicos que no puedes programar.

### Tamaño tras una racha de pérdidas

Nocional de entrada mediano agrupado por lo que vino antes de la operación: tras una ganancia, tras una pérdida, tras dos, tras tres o más. Cada fila lleva su recuento de operaciones, win rate, expectancy, R medio y neto, así que la escalada se valora, no solo se nota.

Subir el tamaño tras dos pérdidas es el hábito más caro que detecta un journal. Una fila plana aquí es la disciplina que la mayoría de libros pierde primero.

### Ritmo tras una pérdida

El intervalo mediano desde una salida hasta la siguiente entrada, comparado tras una ganancia y tras una pérdida. Una operación abierta en mucho menos que **tu propio** intervalo habitual justo después de una pérdida se cuenta como revenge trade, ya que un scalper y un swing trader no comparten reloj. Esas operaciones tienen su propia línea: cuántas, qué hicieron y qué promedian frente a todo lo demás.

También se comparan los días, un día con una pérdida frente a uno limpio, en número de operaciones.

### Cómo va el día

Resultado medio por el orden de la operación dentro de su día local: primera, segunda, tercera, cuarta y siguientes, con R medio por orden y lo que vale cada operación adicional del día. Muchos libros ganan su dinero antes de comer y lo devuelven después. Aquí es donde se ve.

### Tras una ganancia, tras una pérdida

La operación que **sigue** a un resultado, nunca el resultado en sí. Operaciones, win rate, expectancy, R medio, tamaño mediano, riesgo medio, tenencia mediana e intervalo mediano, lado a lado, con las tres diferencias que importan (expectancy, tamaño, tenencia) destacadas debajo.

::: tip Las afirmaciones tienen un umbral mínimo
La frase en la parte superior de una tarjeta solo se escribe por encima de un **umbral de muestra** (veinte operaciones cerradas en el ámbito, ocho a cada lado de una comparación) **y** un umbral de efecto. Por debajo de cualquiera de los dos, las tarjetas se dibujan igualmente y se etiquetan como un primer vistazo. Tres operaciones no pueden mostrar un hábito.
:::

## Enriquecimiento con datos de mercado {#market-data}

El registro de operaciones conoce tu entrada, tu salida y tu stop. No sabe adónde fue el precio mientras estabas dentro, y ahí está la mayoría de las respuestas útiles: si tus stops están dentro del ruido, cuánto de cada movimiento te quedaste realmente y en qué condiciones de mercado funciona la estrategia.

La pestaña **Datos de mercado** carga las velas detrás de tus propias operaciones y las mide.

### Configúralo una vez

Abre **Fuentes** en la pestaña:

- **Tamaño de vela**: *Automático* elige el timeframe más grueso que aún deja unas veinte velas dentro de una posición típica, leído de tu propio tiempo de tenencia mediano. Fija uno si prefieres decidir tú.
- **Obtención**: *Desactivada* mide solo lo ya almacenado, *Bajo demanda* descarga cuando haces clic, *Automática* encola por sí sola la ventana que falta de una operación nueva y te avisa cuando llega.
- **Fuente por tipo de activo**: acciones, ETF, crypto, forex y futuros eligen cada uno un [connector de datos](/es/config/connectors) concedido al journal, o *Automático*, que toma el primer connector concedido que sirva ese tipo.

No se descarga nada a tus espaldas, y las descargas son jobs normales de [Datos históricos](/es/modules/market-data#histdata): misma cola, misma contabilidad de cuota, misma lista de jobs.

### Descubrir, descargar, medir

Tres botones, en ese orden.

- **Descubrir** lee lo que necesitan las operaciones filtradas frente a lo que ya almacenas, y no escribe nada. Por instrumento obtienes las operaciones en el ámbito, las barras almacenadas, las ventanas que faltan y un estado: *listo*, *parcial*, *falta*, *sin fuente*, *no admitido*, *contrato necesario*.
- **Descargar lo que falta** encola esas ventanas y sigue el lote. Solo se piden los huecos, y un hueco se pide a partir de las barras en lugar de adivinarlo por un intervalo: una serie diaria de acciones carece de cada fin de semana, una serie crypto 24/7 nunca, así que ningún ancho de hueco sirve para ambas.
- **Medir** recorre cada operación contra sus barras y almacena el resultado.

Medir es **incremental**. Una medición almacenada se rehace cuando se editó la operación, cuando llegaron velas nuevas o cuando cambió el grano. *Volver a medir todo* fuerza todo el ámbito, para cuando cambias el tamaño de vela y quieres todas las operaciones en igualdad de condiciones.

### Nombrar un contrato de futuros

Una acción se llama igual en todas partes. Un contrato de futuros no, y el ticker de tu journal suele nombrar la raíz que operas y no el contrato que sirve tu proveedor de datos.

Por eso el journal pregunta una vez, en lugar de adivinar. Un instrumento que lo necesita muestra *contrato necesario*, y **Nombrar el contrato** toma el símbolo tal como lo escribe tu fuente: `MNQU6` (lo que muestra TWS y lo que copias), o la raíz con su mes de contrato, `MNQ.202609`, o `MNQ.202609@CME` cuando la raíz cotiza en varios exchanges. Todo lo posterior usa ese símbolo.

Las opciones se informan como **no admitidas** en lugar de emparejarse con su subyacente. Medir una operación de opciones contra las velas de la acción produciría números que parecen correctos y no significan nada.

### Qué obtienes

- **Excursiones**: MAE y MFE medios, en dinero y en unidades del riesgo planificado, con el tiempo mediano desde la entrada hasta cada uno.
- **Eficiencia de salida**: el porcentaje del mejor movimiento que realmente conservaste, y lo que se dejó sobre la mesa en todas las operaciones medidas.
- **¿Son los stops demasiado ajustados?**: distancia mediana del stop en ATR en la entrada, cuántos stops están por debajo de un ATR, y cuántas *ganadoras* pasaron antes del 80 % de su riesgo. Un stop dentro del ruido es un stop que el mercado se lleva de camino a tu objetivo.
- **¿Están los objetivos demasiado cerca?**: ganadoras que conservaron menos de la mitad del movimiento ofrecido, y lo que se mostraba en el mejor punto frente a lo que llegó a casa.
- **Cuánto hay que aguantar antes de que funcione**: el peor punto de cada operación agrupado en R, de 0 a 0,25R hasta más de 1,5R. Esto te dice dónde corresponde un stop.
- **Por régimen de volatilidad** y **por tendencia en la entrada**: las mismas estadísticas divididas en calma / normal / volátil, y al alza / plana / a la baja.

Los regímenes son **terciles de tu propio libro**, no umbrales absolutos. Un umbral absoluto llamaría volátil a toda operación crypto y no te diría nada sobre cuándo funciona tu estrategia. Con menos de doce operaciones medidas no se etiqueta ningún régimen.

La pestaña Dispersión también lee esto: MAE frente a R, eficiencia frente a tiempo de tenencia, la nube coloreada por régimen.

## Riesgo abierto {#open-risk}

Todas las demás pestañas miden el pasado. **Riesgo abierto** mide lo que sigue en juego ahora mismo.

Una posición está abierta cuando queda cantidad, y se cuenta por ese **remanente**: una operación recortada en tres cuartos lleva un cuarto del riesgo, no el riesgo con el que se abrió.

### El titular

- **Exposición bruta y neta**, en dinero y como porcentaje de la cuenta, largos y cortos sumados y luego compensados.
- **En juego**: lo que pierdes si se activan todos los stops planificados. Las posiciones sin stop registrado se cuentan aparte y se nombran, ya que lo que arriesgan es desconocido, no cero.
- **Resultado abierto**, valorado al último cierre almacenado, indicando cuántas posiciones se pudieron realmente valorar.
- **Apuestas efectivas**: a cuántas posiciones independientes equivalen tus líneas, por tamaño, y de nuevo con sus correlaciones medidas.

La tabla de posiciones las lista de mayor a menor, con lado, tamaño abierto, entrada, stop, último precio, valor, lo que está en juego, su porcentaje del total, resultado abierto y días de tenencia.

### Dónde está el riesgo

Concentración por instrumento, clase de activo, lado o estrategia, calculada sobre el **riesgo** cuando hay stops registrados y recurriendo al tamaño cuando no (el panel dice cuál). Un instrumento que lleva la mitad de lo que está en juego es un hecho sobre tu libro que ninguna curva de equity muestra.

### ¿Son apuestas separadas?

Cinco líneas que se mueven juntas son una posición con cinco veces el tamaño. Para responderlo, la pestaña mide la correlación sobre **velas diarias ya almacenadas**, sea cual sea el grano que usó el enriquecimiento, y nunca obtiene nada.

Tres números, y solo el tercero describe tu libro:

- **Sumado**: todos los stops activados a la vez, sumados.
- **Si fueran independientes**: cuál sería el riesgo si nada se moviera junto.
- **Con estas correlaciones**: lo que el libro realmente arriesga.

Su cociente es el factor de **apilamiento**: 1,0 significa apuestas genuinamente separadas, más alto significa la misma apuesta varias veces. Los instrumentos sin velas almacenadas se nombran y se dejan fuera de la matriz en lugar de asumirse.

Los avisos se leen como frases: un par moviéndose a 0,9, un solo instrumento con demasiado peso, posiciones sin stop, un libro más delgado de lo que parece. Cuando no hay nada mal, también se dice.

## Etiquetas de disciplina

Una **etiqueta** es una regla que rompiste o respetaste: *moví mi stop*, *sin setup*, *subí tamaño tras una pérdida*. Márcalas en las operaciones donde aplican y Análisis las valora: cuántas operaciones cerradas rompieron una regla, cuánto promedian frente a las limpias, y la diferencia entre ambas. Ese es el **coste de los errores**, en dinero.

## Calendario PnL

Una cuadrícula mensual del PnL realizado diario, verde para días al alza y rojo para días a la baja, escalada al mayor día del mes, con totales semanales al lado. Haz clic en un día para saltar a sus operaciones.

También lee tus **rutinas de trading**. Adjunta las rutinas que sigue una categoría, durante un periodo, y cada día operado lleva un punto: verde cuando se marcó toda rutina que tocaba ese día, rojo cuando no se marcó ninguna, ámbar en medio. Un día que no operaste queda gris digan lo que digan las rutinas, y un día sin ninguna rutina pendiente no recibe punto alguno. Al pasar el cursor, cada rutina aparece con su propia marca.

Las rutinas en sí viven en [Rutinas de trading](/es/modules/productivity#routines) y se marcan allí: el journal solo registra cuáles sigue un libro, así que el mismo hábito nunca se escribe ni se marca dos veces.

## Importar un libro de operaciones {#import-a-trade-book}

La vista **Importar** toma un libro de operaciones que llevas en otro sitio (una exportación de broker, otro journal, una hoja de cálculo) y lo convierte en operaciones del journal. Sin parser por broker: CSV, TSV, JSON y Parquet pasan por la misma detección.

**Cómo funciona.** Suelta el archivo, el servidor propone un mapeo, lo compruebas contra una vista previa de operaciones reales y luego importas.

- La **detección** lee las cabeceras (en, fr, es, de, it, pt, más la redacción habitual de brokers) *y* los propios valores. El delimitador, el separador decimal y las fechas día-primero frente a mes-primero se deciden por columna. Una columna que no puede identificar con confianza queda **sin mapear** en lugar de adivinarse, y la asignas tú.
- **Vista previa antes de escribir.** El paso de análisis no escribe nada: devuelve las operaciones construidas, los totales y los errores por fila, y se vuelve a ejecutar en cada edición del mapeo, así que lo que ves es exactamente lo que se guardará. El P&L del propio archivo se contrasta con el calculado.
- **Forma de fila.** Una fila es un **round trip** (una fila = una operación) o una **ejecución** (una fila = un fill). Las ejecuciones se agrupan por instrumento en posiciones con tramos de entrada y salida; lo que siga abierto al final se importa como operación abierta.
- **Extractos apilados.** Una exportación que empaqueta varias tablas en un archivo (al estilo IBKR) se lee sección a sección, con un selector para cambiar de tabla o leer el archivo plano.
- **Valor del punto.** Para cada ticker encontrado en el archivo, la importación pide el valor del punto del contrato, ya que ninguna exportación lo lleva. Se guarda con el mapeo.
- **Mapeos.** Guarda un mapeo y el siguiente archivo de la misma fuente se reconoce por la huella de su cabecera y se mapea solo. Las columnas que corriges a mano también se recuerdan.

::: tip Un mapeo no es una plantilla
Una **plantilla** del journal es el formulario con el que registras una operación a mano. Un **mapeo** de importación dice qué columna de un archivo ajeno es qué campo de la operación. Se listan por separado y nunca se mezclan.
:::

### Consultar desde un broker

Con una [cuenta de broker](/es/config/brokers) concedida al journal, **Consultar desde un broker** hace la misma importación sin archivo: elige la cuenta y un periodo, y los fills vuelven plegados en posiciones, con vista previa antes de escribir nada. No hay mapeo que comprobar, una API responde campos tipados. Volver a ejecutar un periodo más amplio actualiza las posiciones ya importadas en lugar de duplicarlas.

**Deshacer una importación.** Cada importación es un **lote**, listado con su fecha, archivo y número de operaciones. *Revertir* elimina exactamente las operaciones que creó. Las importaciones también se deduplican **por categoría**, así que reimportar el mismo archivo no cambia nada (el mismo extracto aún puede alimentar dos categorías, ya que una categoría es un libro). *Olvidar* un lote elimina esa protección y vuelve ordinarias sus operaciones.

## Exportación e informes

Desde la pestaña Operaciones puedes exportar tus datos y generar un informe de rendimiento:

- **Exportación CSV**: las operaciones en bruto, para hojas de cálculo o software fiscal.
- **Informe periódico**: un resumen de rendimiento semanal o mensual (win rate, expectancy, comisiones, desglose por estrategia y categoría, curva de equity), generado en **Markdown o PDF**.

## Funciona con

- **Calculadora de Impuestos**: carga el PnL realizado de tu journal para un año fiscal, dividido en ganancias de capital / derivados / crypto.
- **Datos históricos**: las pestañas Datos de mercado y Riesgo abierto leen velas mediante los [connectors de datos](/es/config/connectors) concedidos al journal, y descargan lo que les falta como jobs normales.
- **Rutinas de trading**: el calendario PnL muestra, por día operado, si se marcaron las rutinas que sigue la categoría.
- **Dashboard**: un widget de operación rápida registra una operación desde la página de inicio.
- **RemindMe**: añade recordatorios vinculados al journal (p. ej. revisión semanal).
