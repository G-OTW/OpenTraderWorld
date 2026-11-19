# Bots de trading {#trading-robots}

Un **bot de trading** es una estrategia de Backtest ejecutándose hacia delante. Congela los ajustes de la estrategia, se ejecuta en cada cierre del timeframe de la estrategia sobre un conjunto de instrumentos y, o bien simula sus fills, o bien envía sus órdenes a una cuenta de broker. Por encima de la estrategia, cada bot tiene su propio dinero (una **política de capital**), **exclusiones**, **protecciones** que reaccionan a sus propios resultados y una **prioridad**.

No hay un segundo motor: cada ejecución lee la ventana con el backtest normal y solo actúa sobre la vela que acaba de cerrar. Un bot de señales arrastra las posiciones que tomó tal como las tomó; lo que la ventana muestra en velas anteriores es historial, nunca se registra ni se envía.

La estrategia da las señales, su stop y su objetivo, y sus tipos de orden. El bot da el dinero y el calendario: su [política de capital](#capital-policy) y sus [exclusiones](#exclusions). El instrumento (valor del punto, tick de precio, paso de tamaño, mínimo) es el del venue. El capital, el tamaño, los filtros de días y de sesiones y los costes de la estrategia no se leen.

El módulo necesita que **Backtest** esté instalado. Vive en **/robots**, como **Bots de trading** en el selector de módulos.

## La flota {#the-fleet}

La página de la flota lista todos los bots: tarjetas en un móvil, una tabla ordenable en un escritorio. Se actualiza cada 10 segundos mientras la página está visible, y las cuentas atrás avanzan cada segundo.

- **Cifras clave**: **Bots** (con cuántos están activos), **Capital** (desglosado por bot), **Hoy** (el P&L de hoy y los eventos sin leer), **Abiertas** (posiciones abiertas y cuántos bots tienen alguna).
- **P&L de la flota**: la curva de capital sumada de los bots, con **P&L**, **Realizado** y **No realizado**. Fíltrala por entorno (**Todos los entornos**, Simulado, Papel, Testnet, Real) y por una fecha **Desde** y **Hasta**. Sin fecha final, el último punto se valora con las posiciones abiertas.
- **Clasificación**: los bots ordenados por el P&L de las operaciones cerradas **Hoy**, en **1S**, **1M** o **Todo**, con el rendimiento sobre su capital inicial.
- **Exposición por tipo de activo**: lo que las posiciones abiertas comprometen al último precio, repartido por tipo de activo (cripto, futuros, forex, acciones...), con cuántos bots hay en cada uno.
- **Filtros de estado**: un clic en una etiqueta muestra los bots en ese estado (**Activo**, **Detenido por una protección**, **Detenido**, **Desarmado, vuelva a armarlo**, **Degradado**, **Bloqueado**, **Bróker inaccesible**, **Error**), con un recuento en cada una. Un segundo clic lo quita. El selector de cuenta se limita a **Simulado** o a una cuenta.
- **Requiere atención**: los bots con un error, una protección que los retiene o eventos sin leer, cada uno con el motivo.
- **Próximos ticks**: los ocho bots que vencen antes, con sus instrumentos y una cuenta atrás.
- **Marcar N como leídos** borra los eventos sin leer de todos los bots. **Nuevo bot** abre el asistente.

## Crear un bot {#create-a-robot}

**Nuevo bot** en la página de la flota, o **Ejecutar como bot** en Backtest: sobre un resultado (la ejecución exactamente como se ejecutó) o sobre una estrategia guardada en la biblioteca. En ambos casos se abre el asistente, paso a paso:

1. **Estrategia**: el nombre del bot y una estrategia guardada que congelar (o la ejecución de la que viene). **Crear una estrategia** abre Backtest. Una edición posterior de la estrategia no cambia nada en el bot hasta que lo actualizas (ver [Ajustes](#settings)).
2. **Instrumentos**: **Añadir un instrumento** abre el selector de datos del gráfico, que busca en la búsqueda de símbolos de cada proveedor. No hace falta histórico almacenado: cada ejecución obtiene las velas que lee la estrategia. Hasta 8 instrumentos, en una misma **Temporalidad** que comparten todos. Cada instrumento se lee mediante la fuente de datos desde la que se eligió, que **Ajustes del bot** puede cambiar después. Donde el proveedor emite un precio en vivo, los stops, objetivos y límites se activan con él; en los demás casos se comprueban al cierre de la vela. Los ajustes del bot indican cuál se aplica a cada instrumento.
3. **Cuenta**: **Simular** (no se envía nada a ningún sitio) o **Cuenta de broker** (ver [En una cuenta de broker](#on-a-broker-account)).
4. **Capital**: la [política de capital](#capital-policy).
5. **Protecciones**: las [protecciones](#guards). Un bot nuevo empieza con el conjunto por defecto.
6. **Calendario y notificaciones**: el bot se ejecuta en cada cierre de sus velas y decide sobre la vela que acaba de cerrar. El paso contiene sus [**Exclusiones**](#exclusions), un **Informe de estado** opcional (**Enviar un informe de estado**, y luego cuándo sale: cada N minutos, a diario, semanal, mensual o una vez, en una zona horaria), **Velas móviles** (cuánto histórico recibe el motor en cada ejecución, 5000 por defecto), **Prioridad**, **Notificar** (**Cada ejecución**, **Solo con una nueva operación**, **Nunca**) y **Canales**.
7. **Revisión**, y luego **Crear el bot**.

El servidor vuelve a comprobarlo todo cuando se crea el bot. Un rechazo se muestra tal como se dijo, y nombra qué cambiar.

El bot arranca enseguida. La ventana **Iniciando** *nombre del bot* sigue su primera ejecución mientras descarga su historial y aprende su libro; **Seguir en segundo plano** la cierra y abre la página del bot mientras la ejecución continúa. Una vez listo, un bot simulado se ejecuta en cada cierre a partir de entonces, y un bot de broker ofrece **Armar** en la misma ventana (ver [En una cuenta de broker](#on-a-broker-account)). **Abrir el bot** abre su página.

Los canales son los canales de notificación concedidos a Bots de trading; el botón de canales del paso los añade o gestiona sin salir del asistente. Sin ningún canal elegido, el bot envía a todos los canales concedidos a Bots de trading; sin ninguno concedido, no se envía nada y cada evento se registra igualmente en el registro del bot. **Notificar** **Nunca** no envía nada en absoluto. El informe de estado da el estado del bot, sus cifras desde el último informe y lo que tiene; no se envía mientras el bot está excluido y no tiene nada. **Ajustes del bot** también contiene avisos en las entradas y salidas por instrumento, y tu propia redacción para los mensajes de entrada y salida, y para el resumen cuando hay un informe de estado activado.

### Exclusiones {#exclusions}

Las exclusiones son periodos en los que el bot no toma ninguna entrada nueva. Las salidas se siguen gestionando dentro de ellos. Se leen en la zona horaria del bot, y cualquier combinación funciona:

- **Días completos**: los días de la semana elegidos.
- **Rangos de horas**: de una hora a otra, con el final excluido; un rango que termina antes de empezar pasa la medianoche.
- **Rangos de fechas**: de una fecha a otra, ambos días incluidos.

Hasta 20 rangos de horas y 20 rangos de fechas. Con **Salir del mercado**, el bot cierra sus posiciones cuando empieza un periodo, y vuelve a operar cuando termina. La entrada y la salida de un periodo quedan en el registro, y van a los canales del bot cuando tiene una posición.

## La página de un bot {#a-robots-page}

La cabecera mantiene a la vista el estado del bot: su nombre, la estrategia y su versión, insignias para el estado, el entorno de un bot de broker (**Papel**, **Testnet**, **Real**), la protección (en pausa, detenido o la reducción, como x0.5) y, en una cuenta de broker, **Armado** o **Desarmado**. Debajo: la próxima evaluación como cuenta atrás, la última evaluación y la última sincronización.

- **Ejecutar ahora** ejecuta el bot en el acto. **Detener estrategia** pausa su programación; no cierra posiciones ni borra órdenes. **Iniciar estrategia** la reanuda.
- **Insights** lee las operaciones cerradas del bot, ver [Insights](/es/modules/journal#insights).
- Mientras una protección retiene el bot, un aviso dice por qué, con **Reanudar**.

La página se actualiza cada pocos segundos; las posiciones abiertas siguen el precio en vivo donde el proveedor emite uno. Ocho pestañas:

| Pestaña | Muestra |
|---|---|
| **Resumen** | **Capital**, **P&L realizado**, **P&L latente**, **Hoy** (P&L y órdenes enviadas), **Tasa de acierto**, **Factor de beneficio**, **Drawdown máx.**, **Esperanza**; la curva de capital; el último precio de cada instrumento (en vivo, vela almacenada o desactualizado); **P&L por instrumento** y **P&L por dirección**; el calendario de P&L; un gráfico en la temporalidad de la estrategia con entradas y salidas. Un bot de broker también muestra su [panel de ejecución](#on-a-broker-account) |
| **Posiciones** | instrumento, lado, cantidad al tamaño del bot, precio medio, precio (en vivo o almacenado), no realizado, stop, objetivo y antigüedad |
| **Pendientes** | las órdenes de un bot simulado a la espera de la siguiente apertura; las órdenes de un bot de broker activas en el broker |
| **Operaciones** | las operaciones cerradas, 25 por página, con la reducción de la protección donde se aplicó. **No tomados** cuenta las entradas que el bot rechazó y lista las últimas 10, cada una con su motivo |
| **Estadísticas** | las estadísticas de las operaciones, la curva de rendimiento y, para las estrategias de señal, una dispersión 3D y las rachas |
| **Protecciones** | las reglas como frases, lo que retienen ahora y la cronología de las últimas 100 activaciones, restablecimientos y reanudaciones, de la más reciente a la más antigua |
| **Registro** | los últimos 200 eventos: entradas, salidas, rechazos, acciones de protección, errores |
| **Ajustes** | la estrategia, la cuenta, la programación, la prioridad, el capital y las protecciones, con **Ajustes del bot**, la actualización a la versión de la estrategia y **Eliminar este bot** |

El libro de un bot empieza con su primera ejecución: solo se registra lo que ocurre desde la vela que acaba de cerrar. Lo que la ventana muestra antes es historial, nunca está en su libro, sus estadísticas ni sus protecciones.

### Ajustes {#settings}

**Ajustes del bot** edita el nombre, la fuente de datos de cada instrumento (**Predeterminada del proveedor**, o un connector concedido a Bots de trading), las exclusiones, el informe de estado, la ventana, la prioridad, el capital, las protecciones y las notificaciones. La estrategia, los instrumentos y la cuenta están congelados: otra estrategia u otra cuenta es un bot nuevo. La nota **Operado hacia delante** lista los instrumentos y los tipos de orden de entrada y salida de la estrategia: sus señales, su stop y su objetivo se ejecutan tal como se congelaron, mientras que su capital, su tamaño, su calendario y sus costes no se leen.

**Pasar a la versión actual de la estrategia** (visible cuando el bot viene de una estrategia guardada) toma la estrategia guardada tal como está ahora. El libro del bot se vacía y se vuelve a aprender en la siguiente ejecución, y un bot armado se desarma.

Eliminar un bot elimina su libro, la cronología de sus protecciones y su registro.

## Política de capital {#capital-policy}

El dinero del bot dimensiona cada decisión de la estrategia: la estrategia se ejecuta sobre el capital del bot, no sobre el suyo. Todos los campos son opcionales:

| Campo | Significado |
|---|---|
| **Asignación** | el capital con el que opera el bot. Vacío: el capital y el tamaño de la estrategia |
| **Moneda** | la moneda de la cuenta, para leer las cifras |
| **Parte por posición (%)** | una posición vale esta parte del capital del bot. Necesita una asignación |
| **Apalancamiento** | el bot dimensiona con él, hasta 125. Vacío: 1 |
| **Entradas por posición** | cuántas entradas puede tomar una posición, hasta 50. Vacío: 1. Un bot de broker solo toma 1 |
| **Máx. posiciones abiertas** | una entrada por encima no se toma |
| **Máx. órdenes al día** | entradas contadas por día UTC. Las salidas nunca se retienen |

Cada entrada se multiplica después por la reducción de una protección y se recorta al paso de tamaño del venue (nunca hacia arriba). En una cuenta de broker, la **Posición máxima (unidades)** fijada al crear el bot rechaza una entrada mayor en lugar de recortarla.

**Una entrada rechazada nombra su solución**, en la pestaña Operaciones y en el registro:

> the bot already holds 3 position(s), its maximum of 3: the entry on BTCUSDT was not taken. Raise the maximum of positions

Los demás se leen igual: un tamaño por debajo del paso del venue (sube la asignación, la parte por posición o el apalancamiento) o por debajo de su mínimo (sube la asignación, la parte por posición o el tamaño de la estrategia), el máximo de órdenes del día, un tamaño por encima de la posición máxima.

## Protecciones {#guards}

Las protecciones son reglas sobre las **propias operaciones cerradas** del bot, al tamaño del bot. Hasta 10 por bot. Las salidas siempre se gestionan, retenga lo que retenga una protección.

| Cuando | Entonces |
|---|---|
| **pérdidas consecutivas**, **ganancias consecutivas** (un recuento) | **pausa** durante días u horas (de un minuto a 365 días) |
| **drawdown %**, **desde el máximo** o **desde el inicio** | **detener hasta reanudar** |
| **pérdida % en**, **importe de pérdida en**, **pérdidas en**, **ganancia % en** una ventana: **hoy**, **esta semana**, **este mes**, **desde el inicio** (en la zona horaria del bot) | **reducir el tamaño**: **Dividir por** más de 1 y como máximo 10, **Volver al tamaño completo** **tras operaciones**, **tras ganancias**, **tras días** o **a mano** |

El conjunto por defecto que se ofrece a un bot nuevo: pausa de un día tras 3 pérdidas consecutivas, y reducir el tamaño a la mitad tras un drawdown del 10 % desde el máximo, volviendo al tamaño completo tras 5 operaciones.

- Un bot en pausa o detenido no toma ninguna entrada nueva y sigue gestionando sus posiciones. Un bot reducido dimensiona a la baja sus próximas entradas.
- Una pausa termina sola. Una detención espera a **Reanudar**. Una reducción vuelve al tamaño completo según su regla de restablecimiento, en un solo paso.
- Una regla de recuento vuelve a contar desde su última activación: una pausa tras tres pérdidas se activa de nuevo tras tres más. Una regla de nivel se activa cuando se cruza el nivel, y otra vez cuando dejó de cumplirse o su ventana pasó.
- Varias a la vez: una detención gana a una pausa, una pausa gana a una reducción, y una reducción activada con ellas se aplica cuando se reanudan las entradas. Una reducción sobre otra se multiplica, nunca por debajo de x0.1.
- Los porcentajes son del capital inicial del bot (la asignación, o si no el capital inicial de la estrategia). Un drawdown desde el máximo es del capital máximo; una pérdida o ganancia en hoy, esta semana o este mes es del capital al empezar ese periodo.
- **Reanudar** levanta todo lo que retiene una protección. Editar las reglas conserva lo que se retiene ahora.

Cada activación y cada restablecimiento están en la pestaña **Protecciones** y en el registro, y se envían a los canales del bot.

## Prioridad y límites de tasa {#priority-and-rate-limits}

Cada bot tiene una **Prioridad** de 0 a 1000, 100 por defecto. **El valor más bajo se ejecuta primero** cuando varios bots vencen a la vez o comparten un broker. Los bots que vencen en el mismo cierre se ejecutan en paralelo, hasta 8 a la vez.

Los bots en una cuenta de broker se sincronizan cada 15 segundos, broker a broker, por orden de prioridad:

- una pasada envía como máximo 10 órdenes a un broker, bot a bot por orden de prioridad. Las salidas y los cierres nunca se retienen por ese límite; una entrada por encima espera a la siguiente pasada, y el bot lo indica;
- cuando un broker rechaza peticiones por su límite de tasa, **solo esperan los bots de ese broker**: hasta la hora que indicó (una hora como máximo), o durante una pasada si no indicó ninguna. Cada bot muestra una nota que nombra la espera y qué cambiar (menos bots o instrumentos en la cuenta, o un límite más alto en su plan). Los bots de otros brokers siguen.

## En una cuenta de broker {#on-a-broker-account}

Un bot puede enviar sus órdenes a cualquier cuenta con el [trading](/es/config/trading) activado y concedida a **Bots de trading** en [Ajustes → Brokers](/es/config/brokers), incluida una cuenta de [paper trading](/es/config/paper-trading). La insignia de entorno dice adónde van: **Papel** (el broker paper de la app), **Testnet** (la cuenta de prueba o demo de un venue), **Real** (dinero real).

El motor decide en cada cierre y la cuenta ejecuta. Al crear el bot:

- solo una estrategia de señal se ejecuta en un broker, con una entrada por posición (**Entradas por posición** 1);
- cada instrumento se busca en la cuenta, que muestra el símbolo y el paso de tamaño que encontró. Un instrumento que la cuenta no sabe nombrar se rechaza: el bot nunca adivina;
- **Salidas**: **En el broker** (el stop y el objetivo reposan en el broker, adjuntos a la entrada, cuando el broker puede mantenerlos) o **Vigiladas** (comprobadas sobre el precio en vivo y cerradas a mercado al cruzarse);
- varios bots pueden apuntar a un mismo instrumento en una misma cuenta, pero solo uno a la vez está armado, para que dos bots nunca operen una misma posición uno contra otro.

Un bot de broker **se crea desarmado**: no se envía nada hasta que lo **Armas**, en la ventana **Iniciando** *nombre del bot* una vez terminada su primera ejecución, o más tarde desde su página. Armarlo requiere el **trading en segundo plano** firmado en la cuenta (**Firmar el trading en segundo plano** abre las condiciones que firmar), y tu código del autenticador cuando el [bloqueo de trading](/es/config/trading#trading-lock) está activado y la cuenta no es paper. A partir del siguiente cierre, las entradas salen a mercado y las salidas como decide la estrategia, sin nadie vigilando. Cada orden pasa los [límites de riesgo](/es/config/trading#risk-limits) de la cuenta y está en el [registro de auditoría](/es/config/trading#audit-log).

Armar lee primero la cuenta: se rechaza cuando no se puede contactar con el broker, o cuando la cuenta tiene menos que el dinero del bot (su asignación, o si no el capital de la estrategia). Cada bot se comprueba por separado, así que varios bots pueden compartir una cuenta; una entrada que la cuenta no puede pagar después no se envía, y el registro dice qué necesitaba y qué tenía la cuenta.

El **panel de ejecución** del Resumen muestra el nivel de la cuenta, sus límites, el libro del motor junto al de la cuenta, y el precio simulado de cada operación junto al del broker. Su estado:

| Estado | Significa |
|---|---|
| **Desarmado** | no se envía nada hasta que armas el bot |
| **Alineado** | la cuenta tiene lo que el bot espera |
| **Degradado** | una entrada se ejecutó solo en parte. El bot no añade exposición y sigue gestionando sus salidas hasta que cierras todo o adoptas |
| **Detenido, vuelva a armarlo** | se abrió una posición a mano en el instrumento del bot mientras el bot estaba sin posición, o la posición del bot se cerró, aumentó o redujo a mano. El bot se desarmó y se cancelaron sus entradas en espera; también se cancelan las salidas de una posición cerrada. Vuelve a armarlo para continuar |
| **Envío en pausa** | una protección retiene las entradas nuevas, o el trading está detenido en la cuenta: solo se envía lo que reduce el riesgo |
| **Verificar en el broker** | una orden no obtuvo respuesta: no se envía nada más en ese instrumento hasta que se resuelva |
| **Bróker inaccesible** | el broker no respondió a tres sincronizaciones seguidas (45 s). No se envía nada hasta que responda, y la cuenta se vuelve a leer antes de enviar nada |
| **Error de cuenta** | no se puede usar la cuenta (sus credenciales o su concesión): no se envía nada hasta que se pueda |

El bot lee la cuenta en cada sincronización (cada 15 s, y lo primero tras un reinicio) antes de enviar nada. Cualquier cambio que hagas a mano en la posición del bot lo detiene hasta que lo armes de nuevo. Un stop o un objetivo que muevas a mano en el broker se queda donde lo pusiste: el bot ya no lo mueve. Cancela a mano tanto su stop como su objetivo y la siguiente sincronización los vuelve a colocar, una vez; después el bot los vigila sobre el precio en vivo. Cancelar cualquier otra de sus órdenes pendientes no cambia nada para él. El control de posiciones propio de la cuenta deja el instrumento de un bot al bot: las operaciones de un bot nunca detienen la cuenta.

**Posiciones** y **Pendientes** muestran solo lo que está en el broker. Una entrada que no salió (rechazada por un límite, una protección, las reglas de tamaño del venue, los fondos de la cuenta o el broker) es una línea en el **Registro**, con su motivo exacto al pasar el ratón. Los precios y el P&L de una operación cerrada son aquellos a los que se ejecutaron sus órdenes en el broker, comisiones incluidas, una vez ejecutada su salida; hasta entonces vale la cifra simulada. Un stop o un objetivo que se ejecuta en el broker, **Detener y cerrar todo**, o un cierre que haces en el broker cierra la operación del bot en ese precio, aunque las velas del propio bot nunca llegaran al nivel; una entrada que nunca salió no se toma. Una vela que el proveedor corrige tras su cierre no cambia nada: el bot hace lo que decidió en ese cierre, y una inversión es un solo mensaje.

- **Desarmar** deja de enviar. Las órdenes que ya están en el broker se quedan allí.
- **Detener y cerrar todo** desarma, cancela las órdenes activas del bot y cierra a mercado lo que tiene.
- **Adoptar la diferencia** te deja a ti lo que la cuenta tiene fuera del bot, para que un bot degradado vuelva a operar.

Armar, desarmar, cerrar todo y adoptar siempre lo haces tú desde la página, nunca un agent ni un workflow.

## Sesiones paper {#paper-sessions}

Las sesiones paper de Backtest son ahora bots, con su libro, sus eventos, su cuenta y sus concesiones de canales. Un enlace antiguo a una sesión paper abre la página del bot. **Ejecutar como bot** sustituye a **Paper trade** en Backtest.

## Conviene saber {#good-to-know}

- Los [agentes de IA (MCP)](/es/config/ai-agents) pueden leer los bots (la lista, un bot, su panel, sus operaciones, sus eventos de protección, su panel de ejecución, la flota, la bandeja de entrada y las variables de los mensajes). Crear, editar, ejecutar o armar uno sigue haciéndose en la app.
- Un bot lee las velas y el precio en vivo de cada instrumento mediante la fuente de datos elegida para él, si no mediante un [connector de datos](/es/config/connectors) concedido a Bots de trading, si no mediante uno concedido a Backtest, y si no mediante el primer connector del proveedor.
- En [modo demo](/es/guide/demo) el módulo es de solo lectura: dos bots de ejemplo en una cuenta paper de demo, uno reducido por su protección y otro en pausa, ninguno armado ni en ejecución.
- Cómo se ejecuta un bot simulado sobre el precio en vivo, y las opciones que comparte con el backtest: [Backtest](/es/modules/market-data#backtest).
