# Connectors de datos

Todos los módulos que leen datos de mercado se alimentan de **una lista compartida de connectors**. Una cuenta de proveedor se crea una vez y se concede a los módulos que pueden usarla.

Gestiónalos en **Ajustes → Connectors de datos**, en la página independiente **/connectors**, o desde el botón de connector que cualquier módulo de datos pone junto a su selector de proveedor. Los tres muestran la misma pantalla.

## Qué es un connector

Un **connector es una instancia con nombre de un proveedor**, no el proveedor en sí. Le pertenecen cuatro cosas:

- **el proveedor**: Binance, Yahoo Finance, EODHD…;
- **sus credenciales**: escritas, o enchufadas desde la [Bóveda](/es/config/settings#vault). Solo escritura: la app solo sabe *qué* nombres de secreto están definidos;
- **un límite de peticiones opcional**: un número máximo de llamadas por periodo;
- **los módulos autorizados a usarlo**: uno o varios, o *todos los módulos* (un comodín que también cubre los módulos de datos añadidos en versiones futuras).

Pueden coexistir varios connectors del mismo proveedor. Ese es el punto: una clave de solo lectura para los gráficos y otra clave aparte para las descargas masivas, cada una con su propio límite, cada una concedida a un módulo distinto.

## Proveedores

| Proveedor | Credenciales | Tipos de activo | Búsqueda de símbolos | Stream en vivo |
|---|---|---|---|---|
| **Binance** | ninguna | crypto | sí | sí |
| **Binance USDⓈ-M Futures** | ninguna | crypto | sí | sí |
| **Bitget** | ninguna | crypto | sí | sí |
| **OKX** | ninguna | crypto | sí | sí |
| **Kraken** | ninguna | crypto | sí | sí |
| **Coinbase** | ninguna | crypto | sí | sí |
| **OANDA** | `api_token` (+ id de cuenta) | FX y CFD | sí | no |
| **Yahoo Finance** | ninguna | acciones, ETF, índices, crypto | sí | no |
| **Alpha Vantage** | `api_key` | acciones, ETF, crypto, FX | sí | no |
| **EODHD** | `api_key` | acciones, ETF, FX, crypto | sí | no |
| **Alpaca** | `api_key`, `api_secret` | acciones, crypto, opciones | sí | intradía |
| **Massive (Polygon.io)** | `api_key` | acciones, ETF, opciones, futuros, crypto, FX, índices | sí | intradía, plan de pago |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` | acciones, ETF, opciones, futuros, índices | por símbolo | no |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | FX y CFD | sí | no |
| **Capital.com** | `api_key`, `identifier`, `api_password` | FX, índices, acciones, crypto (todos CFD) | sí | sí |
| **Interactive Brokers** | ninguna (host + puerto) | acciones, ETF, crypto, FX, índices, futuros, opciones | sí | intradía |

Los que no requieren clave funcionan en cuanto creas el connector. Cada fila de proveedor enlaza a su propia documentación de API y lleva una nota sobre sus límites de tasa, y un connector que hace streaming lleva además una nota sobre lo que cuesta el live allí.

### Streaming en vivo {#live-streaming}

El alcance en vivo es más estrecho que el de descarga, y a propósito.

- Los exchanges crypto publican un canal de velas por intervalo, así que **todo** timeframe que descargan también lo emiten en streaming, el diario incluido: en un mercado 24/7 la vela diaria *es* el día de época. Bitget y OKX anclan sus propias velas diarias y semanales a la medianoche de Hong Kong, así que tanto la descarga como el feed en vivo piden sus variantes alineadas a UTC, y una serie descargada allí encaja con una descargada en cualquier otro sitio.
- **Tres proveedores no hacen streaming aquí.** OANDA y TradeStation publican precios en vivo sobre una respuesta HTTP de larga duración, y FOREX.com sobre Lightstreamer; ninguno de los tres es el WebSocket que hablan los gráficos en vivo. Sus descargas de historial y su lado de cuenta funcionan; la vela en vivo no.
- Alpaca, Massive e Interactive Brokers publican un solo grano cada uno (barras de un minuto, agregados de un minuto y barras de cinco segundos respectivamente) y el timeframe del gráfico se construye a partir de él. Eso deja disponibles todos los timeframes **intradía** y deja **el diario y el semanal a la descarga**: una sesión de acciones no son 1440 minutos alineados a la época, así que una vela diaria construida así no coincidiría con la que almacena la descarga. El gráfico lo dice en lugar de ocultar el control.
- El live suele venderse aparte del historial. Una clave gratuita de Massive descarga historial y es rechazada en el login en vivo; la clave gratuita de Alpaca emite IEX y el feed indicativo de opciones pero no SIP ni OPRA; Interactive Brokers sirve lo que tu cuenta tenga contratado. Cuando un feed no puede funcionar, el gráfico nombra cuál de esos casos es y se detiene, en lugar de reconectar tras un punto que nunca se pone verde.
- La mayoría de estos proveedores permiten **una conexión en vivo por cuenta**, así que un segundo programa con la misma clave ocupa el asiento. Ese caso se informa como tal y sigue reintentando, ya que se resuelve al cerrar el otro.

**Alpaca** lleva un ajuste para esto: *Market data feed*, `iex` (plan gratuito, el predeterminado) o `sip` (de pago). Selecciona solo el socket en vivo; las descargas no se ven afectadas.

### Los proveedores de derivados crypto

`BTCUSDT` es un par spot **y** un perpetuo, y los dos son series distintas: el perpetuo cotiza con una base respecto al spot y un contrato con vencimiento converge hacia él. Por eso el mercado que contiene un dataset nunca se deduce del ticker.

- **Binance USDⓈ-M Futures** es su propio proveedor junto a Binance, no un ajuste de este. Los contratos se escriben como los escribe el mercado de futuros: `BTCUSDT` para un perpetuo, `ETHUSDT_250926` para uno con vencimiento. Los contratos Coin-M (inversos) no se sirven.
- **Bitget** lleva un ajuste *Market*, `spot` (el predeterminado) o `usdt-futures`, porque escribe el mismo ticker de forma idéntica en ambos libros.
- **OKX** no necesita ajuste: sus propios ids de instrumento indican en qué mercado está un ticker, `BTC-USDT` para spot, `BTC-USDT-SWAP` para un perpetuo, `BTC-USD-241227` para un contrato con vencimiento.

### OANDA

El único proveedor de FX con clave de aquí, y su clave es la de la cuenta: OANDA no emite un token solo para datos de mercado, así que el connector pide el mismo token de acceso personal que usa la [cuenta de broker](/es/config/brokers), más el número de cuenta a través del cual lee precios.

- **Ajustes**: *Account ID* (`001-004-1234567-001`) y *Environment* (`live` o `practice`, que son hosts distintos con tokens distintos).
- **Los instrumentos** se escriben `base_quote`, índices y materias primas incluidos: `EUR_USD`, `XAU_USD`, `SPX500_USD`. *Test connection* informa de cuántos puede cotizar la cuenta.
- **Las velas diarias se fijan a medianoche UTC.** El valor predeterminado de OANDA cambia el día a las 17:00 de Nueva York, que es la sesión FX pero no el día en que se almacena el resto de datasets de aquí, así que el connector pide el UTC.
- El volumen es un **recuento de ticks**, no un tamaño negociado: una mesa de dealing publica cuántos precios hizo, no cuánto cambió de manos.

Cuando un proveedor tiene varios connectors, el control en vivo del gráfico gana un selector de cuenta: dos claves son dos derechos y dos asientos de conexión, así que cuál se gasta es elección tuya, no un respaldo.

### TradeStation

Su alcance de datos de mercado va sobre la propia clave OAuth de la cuenta, así que el connector pide el mismo par de claves API y refresh token que usa la [cuenta de broker](/es/config/brokers). No hay credencial de datos de mercado aparte.

- **Ajustes**: *Environment* (`live` o `sim`).
- **Los símbolos** son los propios de TradeStation: `AAPL` para una acción, `@ES` para el futuro continuo, `ESH26` para un contrato, `$SPX.X` para un índice cash, `MSFT 260116C400` para una opción. Al escribir uno se busca y muestra qué es, que es la forma más rápida de pillar una errata.
- **Las barras llevan la marca de su cierre**, así que el connector resta el intervalo y almacena la apertura, como el resto de series de aquí.
- **Solo se ofrecen 1m, 5m, 15m y 1d.** TradeStation construye las barras intradía desde la apertura de la *sesión*, así que su vela horaria empieza a las 9:30 y no encajaría con la vela horaria de ningún otro sitio de tu biblioteca. Descarga 15m y léelo en cualquier timeframe intradía; el rechazo lo dice.

### FOREX.com (StoneX)

Misma historia: sin credencial de datos de mercado propia, así que inicia sesión con el mismo usuario, contraseña y AppKey que la [cuenta de broker](/es/config/brokers), y las dos comparten una sesión.

- **Un mercado es un número.** La API toma un id de mercado numérico; tú escribes `EUR/USD` y el connector lo resuelve. Cuando un nombre coincide con varios mercados, el error los lista con sus ids, y descargas por id.
- **Sin volumen.** Una mesa de dealing publica precios, no tamaño, así que la columna de volumen es cero en lugar de un número plausible.
- **Una barra diaria es la sesión de la plataforma**, que cambia al cierre de Nueva York, no a medianoche UTC. Ese es el periodo que StoneX realmente negoció y se almacena como tal, así que una serie diaria de aquí no se superpone con una de un proveedor de día UTC.
- `4h` se rechaza: StoneX no dice dónde empieza a contar uno. Descarga `1h` y léelo en 4h.

### Capital.com

El tercero con clave cuya clave es la de la cuenta: Capital.com no emite credencial de datos de mercado, así que el connector inicia sesión con la misma clave API, login y contraseña personalizada que la [cuenta de broker](/es/config/brokers), y las dos comparten una sesión.

- **Ajustes**: *Environment* (`live` o `demo`).
- **Un instrumento es un epic**, el nombre de mercado propio de Capital.com: `EURUSD`, `US500`, `AAPL`, `BTCUSD`. La búsqueda de símbolos los devuelve.
- **Una vela es el punto medio de los dos lados** en los que opera la mesa, tanto en la descarga como en el gráfico en vivo.
- **Una barra diaria es la sesión de la plataforma**, no el día UTC, así que una serie diaria de aquí no se superpone con una de un proveedor de día UTC.
- **El live** va sobre la misma sesión y permite 40 instrumentos a la vez. Capital.com emite el bid y el ask como dos velas separadas, así que un panel se llena cuando ambos lados han hecho tick.

### Interactive Brokers {#interactive-brokers}

El caso aparte: no hay URL de proveedor ni clave API. Ejecutas **IB Gateway** o **TWS** en tu propia máquina y el connector habla su protocolo de socket, así que lo que lleva es una **dirección**, no una credencial: un host y un puerto, almacenados en claro para poder diagnosticar una conexión fallida. Los datos son los que tu cuenta de IB tenga contratados.

- **Ajustes**: *Gateway host* (`host.docker.internal` para un gateway en la misma máquina, ya que OpenTraderWorld se ejecuta en un contenedor) y *API port* (4001 live / 4002 paper para el Gateway, 7496 / 7497 para TWS).
- **En el gateway**: Global Configuration → API → Settings, marca *Enable ActiveX and Socket Clients* y comprueba que el puerto coincide. En Docker Desktop la llamada llega desde el loopback del host, así que *Allow connections from localhost only* ya la cubre; en Docker Engine desmárcalo y añade `172.28.53.10` a *Trusted IPs*, que admite direcciones sueltas y no un rango.
- **Test connection** informa de lo que respondió, y nombra el ajuste a cambiar cuando nada responde.
- **Tickers**: `AAPL`, `SAN:EUR` o `7203@TSEJ:JPY` para acciones, `EURUSD` para un par cash, un símbolo OCC para una opción. Un futuro se escribe con su mes, `ES.202512`, o con el símbolo local que muestra TWS, `MNQU6`. Los futuros se buscan en el gateway antes de descargar nada, así que el exchange es opcional: cuando el ticker nombra más de un listado, el error los lista y eliges.
- El client id lo elige la app en una banda privada alta, nunca se pide, así que nada más de lo que tengas conectado al gateway resulta expulsado.
- Interactive Brokers permite 60 peticiones históricas por cada 10 minutos móviles **por cuenta**: el connector se autorregula, así que un backfill largo es lento por diseño.

Probado con **IB Gateway build 10.50.1e (25 ago 2026)**. Se espera que las builds anteriores funcionen, al negociarse a la baja el protocolo de socket, pero esa es la versión con la que se verificó este connector.

## Crear uno

1. **Añadir connector**, elige el proveedor y ponle nombre: el nombre es lo que muestran los selectores de los módulos, así que *Binance gráficos* es mejor que *Binance 2*.
2. Rellena las credenciales que requiere el proveedor, o elígelas de la Bóveda. Los proveedores sin clave se saltan esto.
3. Elige los **módulos** a los que sirve. Si se abre desde un módulo, el nuevo connector se concede solo a ese módulo; si se abre desde Ajustes o `/connectors`, se concede a todo.
4. Opcionalmente define un **límite de peticiones** (ver más abajo).

Un connector al que le falta una credencial requerida se muestra como *necesita credenciales* y todos los módulos lo omiten hasta que la definas.

## Concesiones a módulos

La lista de concesiones está **en el servidor**: un módulo que pide un connector que nunca se le dio es rechazado, así que una casilla que solo viviera en el navegador habría sido decoración. Marcar todos los módulos de datos vuelve al comodín *todos los módulos*, que mantiene cubiertos los módulos de datos futuros.

Los módulos concedibles hoy:

| Módulo | Qué lee |
|---|---|
| **Datos históricos** | la lista de proveedores del formulario de descarga y la búsqueda de símbolos |
| **Visualización** | la búsqueda de símbolos del gráfico, sus ventanas bajo demanda y su stream en vivo |
| **Watchlists** | la fuente de cotizaciones de una lista, o de un solo símbolo |
| **Journal** | las velas detrás de las pestañas Datos de mercado y Riesgo abierto |
| **Herramientas Cuant** | las pestañas de Derivados: contratos de futuros, cadenas de opciones y volatilidad implícita, desde Interactive Brokers o Massive |

## Límites de peticiones {#request-limits}

Un límite es un recuento de llamadas salientes por **día**, **hora** o **minuto**, seguido por connector.

- En todos los demás sitios de la app es **solo de observación**: alimenta los contadores de [Ajustes → Tasa de API](/es/config/settings#api-rate) y te avisa, pero no se limita nada.
- En las consultas bajo demanda del gráfico (`/api/histviz/series`) **bloquea**: cuando el connector alcanza su límite, la ventana vuelve con las barras ya almacenadas y un aviso de *límite de peticiones alcanzado*, en lugar de gastar sin darte cuenta un plan medido.

Deja el límite sin definir si prefieres que sea el proveedor quien diga que no.

## Dónde se usan los connectors

- **Datos históricos**: la lista de proveedores del formulario de descarga y la búsqueda de símbolos.
- **Visualización**: la pestaña Datos busca en todos los connectors concedidos al gráfico a la vez; el stream en vivo corre sobre el connector que elijas en el control en vivo, o sobre el más antiguo que tenga concedido el gráfico para ese proveedor.
- **Watchlists**: la fuente de cotizaciones de una lista, o de un solo símbolo. CoinGecko y Yahoo siguen disponibles sin ningún connector.
- **Trading Journal**: las velas detrás de las pestañas Datos de mercado y Riesgo abierto, con una fuente elegible por tipo de activo.
- **Herramientas Cuant**: las pestañas de Derivados listan los contratos de futuros de un producto o la cadena de opciones de un subyacente y cotizan cada uno. Solo Interactive Brokers y Massive los listan; el historial de volatilidad implícita viene solo de Interactive Brokers.

::: tip Actualizando desde los antiguos ajustes por módulo
La pestaña *Ajustes* de Datos históricos y la pestaña *Fuentes* de Watchlists ya no existen: eran dos copias desconectadas de esta pantalla sobre dos listas desconectadas. Las cuentas creadas en cualquiera de ellas son ahora connectors aquí, cada una aún concedida al módulo del que vino, así que el alcance no cambia al actualizar. Los nombres vuelven a ser únicos globalmente: un nombre que existía en ambas listas se conserva una vez y el otro pasa a llamarse `<name> #2`.
:::
