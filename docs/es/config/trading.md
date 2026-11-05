# Operar desde el gráfico

Una cuenta de broker puede hacer algo más que leer su libro: con el **trading** activado, el gráfico le envía órdenes, las mueve y las cancela desde su línea, y cierra posiciones desde la suya. Hoy aceptan órdenes tres brokers:

- **Interactive Brokers (Gateway)**, a través del IB Gateway o la TWS que usted mismo ejecuta. Esta página.
- **Binance** y **Binance USDⓈ-M Futures**, en cuentas reales y en sus entornos de prueba (Spot Demo Mode, el testnet de futuros). Una cuenta real acepta órdenes una vez activado el [trading real](#live-trading) en ella.
- **Paper trading**, una cuenta simulada que gestiona la propia aplicación. Vea [Cuenta de simulación](/es/config/paper-trading).

Cualquier otro broker responde que no acepta órdenes de OpenTraderWorld y sigue siendo de [solo lectura](/es/config/brokers).

::: danger Experimental
Operar desde el gráfico es **experimental**: por ahora funciona con Interactive Brokers, Binance y Binance USDⓈ-M Futures. Está ampliamente probado, con mercados simulados y en los entornos de prueba de los brokers, pero pueden quedar errores. Compruebe cada orden en su broker, empiece con poco y opere bajo su propio riesgo.
:::

::: tip Cuentas reales
Interactive Brokers, Binance y Binance USDⓈ-M Futures están calificados **E4**: una cuenta real acepta órdenes una vez que active el [trading real](#live-trading) en ella, con sus límites configurados. Las cuentas paper y los entornos de prueba no lo necesitan.

Cancelar órdenes nunca se bloquea, sea cual sea la cuenta.
:::

## Configuración

### 1. El Gateway

Ejecute IB Gateway (o la TWS) con la sesión iniciada en su cuenta paper o real y, en sus ajustes de API:

- marque **Enable ActiveX and Socket Clients**;
- desmarque **Read-Only API**, o IB rechazará cada orden;
- con Docker Engine (Linux), desmarque **Allow connections from localhost only** y añada `172.28.53.10` a **Trusted IPs**.

El client id lo elige la aplicación (3100 a 3115), así que varias sesiones pueden compartir un Gateway.

### 2. La cuenta

En **Ajustes → Brokers**, **Añadir cuenta**, elija **Interactive Brokers (Gateway)** y rellene:

| Ajuste | Valor |
|---|---|
| **Gateway host** | `host.docker.internal` si la aplicación corre en Docker, `127.0.0.1` en otro caso |
| **Gateway port** | `4002` Gateway paper, `4001` Gateway real, `7497` TWS paper, `7496` TWS real |

No hay secreto que dar: el Gateway ya tiene la sesión iniciada. **Probar conexión** comprueba que responde.

### 3. El interruptor de trading

Cada fila de cuenta tiene un chip **Trading**, junto a sus chips de módulos. Está separado a propósito:

- **Todos los módulos** no lo incluye. Leer su libro y enviar órdenes son dos permisos distintos.
- **Activarlo pide su contraseña, cada vez.** Desactivarlo no pide nada.
- Las cuentas de paper trading son la excepción: sin contraseña, porque no hay dinero real en juego.

### 4. El gráfico

Un gráfico opera cuando se cumplen tres cosas:

1. hay una cuenta elegida en el **Broker book** del gráfico;
2. el trading está activado para esa cuenta;
3. los datos del gráfico vienen del mismo broker: una cuenta IBKR opera un gráfico alimentado por el conector de Interactive Brokers.

Cuando falta una, el panel de órdenes dice cuál y cómo arreglarlo. Los futuros necesitan el **mes del contrato**: un futuro continuo se rechaza, así que abra `MNQZ6` en lugar del gráfico continuo.

## El bloqueo de trading {#trading-lock}

Un segundo factor opcional para las órdenes. Con el bloqueo activo, la primera orden tras iniciar sesión pide el código de su autenticador, una vez, y todas las cuentas de trading quedan abiertas en ese navegador hasta cerrar sesión.

- Protege enviar, modificar y cerrar. Cancelar, una orden o todas, nunca pide nada.
- Cubre todas las cuentas de trading salvo las [cuentas paper](/es/config/paper-trading).
- Actívelo en **Ajustes → Seguridad → Bloqueo de trading**, una vez configurada la autenticación en dos factores. Detalles, códigos erróneos y cómo restablecerlo: [Seguridad de la cuenta](/es/config/security#trading-lock).

::: danger Quitar el segundo factor con el bloqueo activo desactiva el trading en todas partes
Quitar la autenticación en dos factores, desde Ajustes o con `disable-totp`, desactiva el bloqueo. Si el bloqueo estaba activo, también desactiva el trading en **todas** las cuentas, paper incluidas. Vuelva a activarlo cuenta por cuenta después.
:::

## Enviar órdenes

### El ticket

La pestaña **Ticket** del panel de órdenes envía una orden, un bracket o un OCO. **Ofrece lo que acepta el broker de la cuenta, y nada más**: un tipo, una vigencia o una opción que el broker no declara no se muestra (vea [Qué acepta cada broker](#what-each-broker-takes)).

- **Tipos:**
  - **Market**, **Limit**, **Stop**, **Stop limit**.
  - **Trailing stop**: un stop que sigue al mercado a una distancia, como porcentaje o importe de precio, y nunca retrocede. **Start at**, donde el broker lo tiene, le hace esperar un precio antes de empezar a seguir.
  - **MIT** (market if touched) y **LIT** (limit if touched): el espejo de un stop, que se activa cuando el mercado llega al precio por el lado del limit (una compra por debajo del mercado, una venta por encima), y luego una orden a mercado o limit.
  - **OCO**: un limit y un stop del mismo lado, donde el primero en ejecutarse cancela el otro. Sus dos precios deben ser distintos.
- **Salidas:** los interruptores **Take profit** y **Stop loss** añaden la orden opuesta por la misma cantidad. Al activar uno se rellena a 0,5 % (take profit) o 0,25 % (stop loss) de la entrada. **Exits time in force** elige GTC (por defecto) o Day. Donde el broker acepta órdenes reduce-only, las salidas se envían reduce-only. No disponible en un OCO, ni en un broker que no adjunta salidas. En Interactive Brokers la entrada se retiene hasta que sus salidas son aceptadas, así el bracket sale completo.
- **Vigencia:** Day, GTC, GTD con fecha y hora futuras, IOC, FOK (todo o nada, de inmediato).
- **Precios:** como precio, o como distancia al bid, al ask o al último (a la entrada, para las salidas), en ticks cuando se conoce el tick.
- **Tamaño:** en **unidades** (acciones, contratos), como **importe** de dinero (dividido por el valor de la entrada), o como el dinero en **riesgo** hasta el stop loss (dividido por la distancia de la entrada al stop loss, que debe estar activado). Un future cuenta el valor de su punto; contratos y acciones se redondean a la baja a unidades enteras.
- **Opciones**, cada una mostrada solo donde el broker la acepta:
  - **Reduce only**: solo reduce la posición, nunca abre ni da la vuelta a una.
  - **Post only**: un limit que solo reposa en el libro; se rechaza en lugar de ejecutarse de inmediato.
  - **Iceberg**: el tamaño mostrado en el libro, el resto oculto.
  - **Trigger on**: el precio que vigila un stop o un disparador (último, mark, bid/ask, mid).
  - **Close position**: un stop o MIT que cierra toda la posición al dispararse, sea cual sea su tamaño.
  - **Outside RTH**: también se activa y se ejecuta fuera del horario regular.
  - **Adaptive algo**: el broker trabaja una orden a mercado o limit para un mejor precio, paciente, normal o urgente.

Antes de enviar nada, la aplicación comprueba:

- que cada precio y cantidad sea mayor que cero, y que cada opción encaje con el tipo (post-only en un limit, un iceberg por debajo del tamaño de la orden, un porcentaje de trailing por debajo de 100);
- que el broker acepte el tipo, la vigencia y cada opción pedidos: lo que no acepta se rechaza, nombrándolo;
- que el precio esté en el tick del instrumento; si no, se rechaza con los dos precios válidos más cercanos (`… is not on MNQZ6's 0.25 tick: use … or …`);
- que el instrumento se resuelva en el broker. **Nunca se adivina.**

Si una pata de un ticket se rechaza, se cancela todo el ticket.

### Las órdenes que la aplicación sigue {#followed-orders}

Cada orden se guarda con su propio id **antes** de enviarse, y la respuesta del broker se guarda con ella. Por tanto:

- **Un id de orden se envía una sola vez por cuenta, para siempre.** El gráfico da a cada orden un id nuevo; un ticket reenviado (tras indicar un margen, por ejemplo) sale con ids nuevos. Un id ya usado se rechaza, incluso tras un reinicio.
- **No tener respuesta no es un rechazo.** Una orden que el broker no confirma a tiempo (10 segundos para el Gateway) se busca en el broker por su id. Cuando aún no aparece, se marca como **desconocida** y **nunca se reenvía**: las órdenes nuevas sobre ese instrumento esperan a que una búsqueda lo resuelva. Si sigue sin resolverse tras 5 minutos, recibe una notificación (canal de notificaciones del gráfico) que le pide comprobar la orden en el broker.
- **Una orden que el broker nunca recibió** (sigue sin aparecer tras las búsquedas) espera a que usted decida. La pestaña Órdenes muestra *the broker never received this order* con **Send again**, que la envía con ids nuevos pasando por todas las comprobaciones de un ticket nuevo, y **Dismiss**, que no envía nada. También recibe una notificación.
- **Un reinicio no olvida nada.** Las órdenes enviadas justo antes de que la aplicación se detuviera se buscan al arrancar, y las órdenes que la propia aplicación enlaza (brackets de Binance USDⓈ-M) se retoman.

Después la aplicación compara su vista con la del broker, unos segundos tras el arranque y en ciclo (cada minuto mientras tiene órdenes fuera, cada 10 minutos en caso contrario, y de inmediato cuando un flujo en directo avisa de eventos perdidos). **Gana el broker**: una orden que se ejecutó o canceló mientras nadie miraba toma el estado del broker, y una orden colocada en otro sitio (TWS, la web del mercado) se anota como externa. Una posición que cambió sin ejecuciones que lo expliquen es una **deriva**: se escribe en el registro de auditoría y se notifica.

### Qué acepta cada broker {#what-each-broker-takes}

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper |
|---|---|---|---|---|
| **Tipos** | todos | todos | todos | todos |
| **Vigencia** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK |
| **Salidas, OCO** | enlazadas por IB | listas de órdenes (OTO, OTOCO, OCO) | enlazadas por la aplicación | enlazadas por la aplicación |
| **Trailing stop** | importe o %, desde el mercado | %, en pasos de 0,01 %, precio de inicio | % (0,1 a 20), precio de inicio | importe o %, precio de inicio |
| **Reduce only** | | | sí | sí |
| **Post only** | | sí (limit maker) | sí (GTX) | sí |
| **Iceberg** | sí, unidades enteras | sí, limits GTC | | |
| **Trigger on** | último, bid/ask, mid | | último, mark | |
| **Close position** | | | sí | |
| **Outside RTH** | sí | | | |
| **Adaptive algo** | sí | | | |

- **Binance spot** retiene las salidas de una entrada **limit** (OTO, OTOCO) y coloca las de una entrada **market** una vez ejecutada; una entrada stop no admite salidas. La mitad limit de un OCO sale como limit maker. Una salida de venta es por la cantidad comprada: una comisión cobrada en la propia moneda deja menos, y Binance rechaza la salida.
- **Binance USDⓈ-M** no adjunta nada por sí mismo: la aplicación envía las salidas como órdenes reduce-only junto a su entrada, y las enlaza, igual que los pares OCO, leyéndolas cada 5 segundos. Tras un reinicio, la aplicación retoma los enlaces a partir de las órdenes que guardó. Se sigue el modo hedge: cada orden nombra su lado long o short, y un símbolo mantenido en ambos sentidos se cierra un lado cada vez.
- Los trailing stops de **Interactive Brokers** siguen desde el momento en que se colocan (sin precio de inicio) y muestran un iceberg en unidades enteras.

### Binance

- **Clave API.** Las órdenes necesitan una clave con el trading habilitado («Enable Spot & Margin Trading» para spot, «Enable Futures» para USDⓈ-M) y los retiros desactivados. En una cuenta real, antes de la primera orden de una sesión, la aplicación lee los permisos de la clave y se niega a enviar cuando el trading está desactivado, los retiros están activados o la clave no está restringida a IP de confianza. Después hace una prueba en seco de la orden en la ruta de pruebas de Binance.
- **Comprobaciones antes de enviar.** Cada orden se somete a los filtros del símbolo tal como los publica Binance: tick, paso y límites de tamaño, tamaño de las órdenes a mercado, valor mínimo y máximo de la orden, la banda de precio alrededor del precio de referencia actual (mark price en futuros) y el tope de órdenes abiertas por símbolo. Una orden rechazada nombra el filtro y los valores más cercanos que pasarían. No se cambia nada por usted.
- **Ritmo de órdenes.** Cuando se superaría el límite de órdenes de la cuenta, la orden se rechaza indicando los segundos que hay que esperar. Nunca se pone en cola.
- **Cambiar un stop de futuros.** Binance no puede modificar una orden condicional en su sitio. La aplicación coloca primero el stop modificado y luego cancela el antiguo, de modo que siempre hay un stop activo. La orden recibe un id nuevo (`….r1`).
- **Si la aplicación desaparece (USDⓈ-M).** Mientras tiene órdenes activas en un símbolo de futuros, la aplicación mantiene armada la cuenta atrás de cancelación automática de Binance (120 s, renovada cada 30 s). Si deja de renovarla, Binance cancela todas las órdenes abiertas en ese símbolo, incluidas las colocadas en otro sitio.
- **Posiciones.** Las posiciones de futuros muestran el apalancamiento y el modo de margen (cross o isolated) fijados en Binance. La aplicación solo los lee.
- **Pestaña Cuenta (spot).** El efectivo es el saldo en USDT; el disponible es su parte libre. El equity es el valor que da Binance a la cartera spot en USDT, solo en real (Spot Demo Mode no lo publica).

### Interactive Brokers

- Un tamaño que el contrato no acepta (por debajo de su mínimo, o fuera de su paso de tamaño, por ejemplo 0,5 de un future) se rechaza antes de enviar nada, indicando los tamaños válidos.
- Una cancelación que llega después de que la orden se ejecutó no es un error: la orden aparece como ejecutada.
- Mientras IB informa de que perdió sus servidores (1100), se rechazan las órdenes nuevas y los cambios; las cancelaciones siguen pasando.
- En una cuenta real, la primera orden de cada conexión al Gateway se envía antes a IB como what-if. Si IB la rechaza, o dice que la cuenta no puede soportarla, el ticket se rechaza con el motivo de IB.

El ticket se acopla abajo, arriba, a la izquierda o a la derecha del gráfico, o entre filas o columnas de un diseño. Arrastre su borde interior para cambiar su tamaño (también con las flechas, Mayús para pasos mayores); el tamaño se guarda por posición.

### La barra rápida

La **barra rápida de Compra/Venta** (botón de la barra de herramientas) muestra Vender, una cantidad y Comprar con el bid y el ask. **Un clic envía una orden a mercado al instante, sin confirmación.** Arrástrela donde quiera; se pega al borde más cercano.

### El `+` de la escala de precios

Pase el ratón por la escala de precios y pulse **+**: el menú ofrece una compra o una venta a ese precio, como limit o stop según el lado del mercado en que esté. Se envía al instante, en Day, o se abre en el ticket con **Abrir en el ticket…**.

## En el gráfico

- **Las órdenes activas** se dibujan como líneas. Arrastre la etiqueta de una línea para mover la orden (se ajusta al tick); un movimiento rechazado devuelve la línea a su sitio. Su **✕** cancela la orden y sus salidas.
- **Doble clic** en una línea de orden, o la pestaña Órdenes, la abre en el ticket para cambiar sus precios, cantidad o vigencia. El lado y el tipo no cambian. Las órdenes enviadas desde la TWS también aparecen y se editan igual si son market, limit, stop o stop-limit.
- **Las posiciones** se dibujan en su precio medio, con una **✕** para cerrar a mercado. Cerrar envía una orden a mercado por la cantidad que el broker mantiene ahora y luego **cancela las salidas de la posición**: las órdenes activas sobre su instrumento, del lado del cierre, que el broker enlaza a una entrada o marca como reduce-only. Las demás órdenes del instrumento se quedan.
- **Proteger con salidas** (el escudo de la pestaña Posiciones) abre el ticket en el gráfico de la posición con un OCO del otro lado por la cantidad mantenida: un take profit y un stop loss, uno cancelando al otro, reduce-only donde el broker lo tiene. La posición debe estar abierta en un gráfico de esa cuenta.
- **Brackets:** al pasar sobre una orden de un bracket se dibujan sus enlaces, verde discontinuo hacia el take profit y rojo discontinuo hacia el stop loss.
- **Ejecuciones:** con **Entry and exit markers** activado, las ejecuciones de la cuenta en el instrumento se dibujan como marcadores en su precio y su barra (una compra apunta hacia arriba, una venta hacia abajo), sobre los últimos 30 días como máximo. Está desactivado por defecto.

**Ajustes del gráfico → Trading** define cómo se dibujan:

| | Opciones |
|---|---|
| **Posición** | Resultado en la línea (ninguno, porcentaje, importe), Línea (ancho completo, corta, solo etiqueta en la escala de precios), mostrar el lado, mostrar la cantidad |
| **Orden** | Línea, mostrar el lado, mostrar la cantidad |
| **Fills** | Entry and exit markers, colores de Buy marker y Sell marker |

## El panel de órdenes

Junto a **Ticket**, filtrado por broker:

| Pestaña | Muestra |
|---|---|
| **Posiciones** | símbolo, lado, cantidad, precio medio, precio de mercado, P&L, hora de apertura y comisiones de entrada (de las ejecuciones de los últimos 7 días), con botón de cierre |
| **Órdenes** | tipo, cantidad, limit, stop y estado, con editar y cancelar. Su **Cancelar todo** cancela las órdenes listadas en este navegador |
| **Cuenta** | las cifras que publica el broker, ver abajo |
| **Registros** | las últimas 200 llamadas enviadas al broker, del registro de auditoría |

### Cifras de la cuenta

**Equity, Efectivo, Resultado abierto, Realizado, Margen inicial, Margen de mantenimiento, Disponible, Exceso de liquidez, Poder de compra, Valor de las posiciones**, en la divisa base de la cuenta, una fila por cuenta que alcanza el Gateway. En Interactive Brokers son las de IB (`NetLiquidation`, `TotalCashValue`, `UnrealizedPnL`, el `RealizedPnL` del día, `InitMarginReq`, `MaintMarginReq`, `AvailableFunds`, `ExcessLiquidity`, `BuyingPower`, `GrossPositionValue`). Una cifra que el broker no publica no se muestra: IB no da un total de comisiones, así que esa línea no aparece.

### Actualizaciones en directo

Órdenes, ejecuciones y posiciones llegan según las comunica el broker, por un único flujo para todas las cuentas de trading de la página: los mensajes propios del Gateway en Interactive Brokers, el user data stream de Binance en Binance (la API WebSocket en spot, la listen key de la cuenta en USDⓈ-M, ambas con la clave ya configurada de la cuenta). Las posiciones de futuros en modo hedge se vuelven a leer enteras en lugar de actualizarse en su sitio. El libro también se relee cada 30 segundos, y un segundo después de que se mueva una posición. Si el flujo se cae, la página pasa a leer el libro cada 4 segundos. Un Gateway que se desconecta se reconecta en la siguiente llamada; entretanto las órdenes activas siguen en IB.

## Límites de riesgo e interruptor de emergencia {#risk-limits}

Sus propios límites, comprobados antes de que salga cualquier orden, en cada cuenta de trading (paper incluida). Cada uno está desactivado hasta que lo fije, en **Ajustes → Brokers → Límites de riesgo** de la cuenta:

| Límite | Rechaza |
|---|---|
| **Orden más grande (tamaño)** | una orden mayor que este número de unidades |
| **Valor máximo de una orden (divisa de la cuenta)** | una orden que valga más que esto, en la divisa de la cuenta (tamaño × precio × valor del punto de un future) |
| **Posición más grande (tamaño)** | una orden que dejaría más de este número de unidades en el instrumento |
| **Valor máximo de una posición (divisa de la cuenta)** | una orden que dejaría una posición que valga más que esto, en la divisa de la cuenta (tamaño en cartera × precio × valor del punto de un future) |
| **Banda de precio (%)** | un limit o stop a más de este porcentaje del último precio del gráfico |
| **Órdenes por minuto** | más órdenes que este número en el último minuto |
| **Órdenes abiertas a la vez** | más órdenes activas que este número |
| **Límite de pérdida por periodo (divisa de la cuenta)** | cualquier orden una vez que la cuenta ha perdido esto desde el inicio del periodo (hora, día o semana, ver abajo): la cuenta se detiene |
| **Ventana de duplicado (segundos)** | la misma orden (lado, tipo, tamaño, precios) enviada de nuevo en estos segundos, hasta que usted confirme |

- Un incumplimiento se rechaza indicando el límite y el valor. **La orden nunca se redimensiona para encajar.**
- **Reducir riesgo siempre pasa**: una orden reduce-only, un cierre o una orden que solo reduce lo mantenido atraviesa todos los límites y todas las detenciones.
- Cambiar un límite pide su contraseña.

### Detenciones

Una detención rechaza órdenes nuevas hasta que termina; las cancelaciones y las órdenes que reducen una posición siguen saliendo, y las órdenes activas no se pueden modificar mientras tanto (cancélelas).

- **Interruptor de emergencia**: el botón de **Ajustes → Brokers** y del panel de órdenes del gráfico cancela todas las órdenes activas de **todas** las cuentas de trading (incluidas las colocadas en otro sitio, cada cancelación en el registro de auditoría) y luego las detiene todas. Pide confirmación, nunca contraseña.
- **Límite de pérdida**: una cuenta que alcanza su límite de pérdida se detiene sola (comprobado antes de cada orden y en el ciclo de conciliación), y recibe una notificación.
- **Deriva de posición**: cuando una posición se mueve sin ejecuciones que lo expliquen (una orden que la aplicación no vio, una ejecución corregida), la cuenta se detiene hasta que la **reconozca**, y recibe una notificación. Compruebe primero la cuenta en el broker; reconocerla siempre pide su contraseña, sea cual sea la política de abajo.

En **Ajustes → Brokers → Riesgo e interruptor de emergencia** elige cómo termina una detención, para todas las cuentas:

- **después**: el inicio de la hora, el día o la semana siguiente (lunes), en la **Zona horaria** de la regla; o **solo cuando se levante**. La zona es UTC hasta que guarde una, y el formulario parte de la zona de su navegador;
- **levantar antes con mi contraseña**: una detención puede levantarse antes desde el mismo sitio o desde el banner del gráfico. Una detención que solo termina al levantarse siempre se levanta con la contraseña.

Guardar la regla pide su contraseña.

El periodo de pérdida sigue la misma elección (un día cuando las detenciones solo terminan al levantarse). La pérdida se mide desde el equity de la cuenta al inicio del periodo, tal como lo indica el broker; un depósito o retirada durante el periodo cuenta. Un broker que no indica equity (Binance Spot Demo Mode) no puede someterse a un límite de pérdida: sus órdenes se rechazan mientras haya uno fijado.

## Trading real {#live-trading}

El circuito de órdenes de cada broker tiene un nivel, que se muestra en la cuenta en **Ajustes → Brokers**: E2 (probado contra un mercado simulado), E3 (probado en el entorno de prueba del propio broker, durante días), E4 (se superaron un ensayo en real y una orden real del tamaño mínimo). Por debajo de E4, el propio conector rechaza una cuenta que opera con dinero real.

En E4, una cuenta real sigue sin aceptar órdenes hasta que active **Trading real** en ella:

- hace falta el interruptor de trading activado, y el **límite de tamaño o valor por orden**, el **límite de tamaño o valor por posición** y el **límite de pérdida** de la cuenta definidos;
- el [bloqueo de trading](#trading-lock) es opcional. Activo, activar el trading real y la primera orden de cada sesión piden el código de su autenticador. Desactivado, la cuenta muestra un aviso y sus órdenes salen sin código;
- la cuenta muestra el nivel, si el broker cancela sus órdenes cuando la aplicación se detiene (Binance USDⓈ-M: sí; spot e Interactive Brokers: no, las órdenes siguen activas) y qué límites están definidos;
- desactivarlo no pide nada; desactivar el trading también lo desactiva, y hay que volver a activarlo.

Los entornos de prueba, las cuentas demo y las cuentas paper nunca lo necesitan.

## Registro de auditoría y métricas de órdenes {#audit-log}

Cada envío, cambio, cancelación, cancelación global y cierre se escribe en un **registro de auditoría de solo anexado**: hora, cuenta, broker, acción, solicitud, respuesta o error del broker y latencia. La base de datos se niega a modificar o borrar una fila. Las llamadas rechazadas antes de llegar al broker porque el trading está desactivado o el bloqueo cerrado no se registran; cualquier otro rechazo sí. Lo que encuentra la [conciliación](#followed-orders) (una ejecución o cancelación perdida, una orden externa, una orden resuelta tras una respuesta perdida, una deriva de posición) también se registra, como **reconcile**, y no cuenta en las métricas de órdenes.

En **Ajustes → Brokers**, cada cuenta de trading muestra sus **métricas de órdenes** (llamadas, enviadas, rechazadas, latencia p50 / p99, última orden) y un botón **Cancelar todas las órdenes**. A diferencia del [interruptor de emergencia](#detenciones), cubre solo esa cuenta y no detiene nada:

- pide confirmación y luego cancela **todas las órdenes activas** de la cuenta en el broker, incluidas las colocadas fuera de OpenTraderWorld. En Interactive Brokers son todas las órdenes de las cuentas del Gateway, incluidas las enviadas a mano en la TWS;
- indica cuántas se cancelaron. En Interactive Brokers espera antes hasta 10 segundos a que no quede ninguna activa;
- funciona con el trading desactivado y con el bloqueo cerrado.

## Límites

- En Interactive Brokers, las órdenes van a la cuenta por defecto del Gateway.
- No se cambia el lado ni el tipo de una orden activa: cancele y envíe una nueva.
- Las órdenes van en la divisa del contrato, sin conversión.
- Los agentes de IA ([MCP](/es/config/ai-agents)) y los workflows del Automator no pueden enviar órdenes.
- Las cuentas de broker, y por tanto el trading, están desactivadas en [modo demo](/es/guide/demo).
