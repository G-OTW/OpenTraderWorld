# Cuentas de broker

Una **cuenta de broker** es una clave de solo lectura al lugar donde realmente operas. Responde tres preguntas que nadie debería reescribir a mano: qué se ejecutó, qué tengo en cartera y qué órdenes tengo activas.

Gestiónalas en **Ajustes → Brokers**, o desde el botón *Brokers* que pone junto a su selector de cuenta cualquier módulo que lea tu libro. Ambos muestran la misma pantalla.

::: warning Solo lectura, por construcción
Las cuentas de broker leen, y solo leen: ninguna ruta detrás de ellas coloca, modifica ni cancela una orden. Las credenciales que se te piden son de tipo solo lectura, así que da exactamente eso: cuando un broker puede emitir una clave de solo consulta, el formulario lo indica. Si algún día se incorpora el enrutamiento de órdenes, será una función aparte, con sus propias claves y su propio permiso, y se indicará aquí.
:::

No lo confundas con los [connectors de datos](/es/config/connectors): esos leen **precios**, estas leen **tu cuenta**. Dos credenciales distintas, dos listas distintas, dos concesiones distintas, a propósito.

## Qué necesitas para conectar

| Broker | Credenciales | Permiso a conceder | Lee |
|---|---|---|---|
| **Alpaca** | `api_key`, `api_secret` | Clave de Trading API del entorno que elijas (live *o* paper, son claves separadas) | fills, posiciones, órdenes, holdings |
| **Binance** | `api_key`, `api_secret` | Solo *Enable Reading*. Sin trading, sin retiradas | fills, órdenes, holdings |
| **Binance USDⓈ-M Futures** | `api_key`, `api_secret` | *Enable Reading* más acceso a futuros. Sin trading, sin retiradas | fills, posiciones, órdenes, saldos de margen |
| **Bitget** | `api_key`, `api_secret`, `api_passphrase` | *Read-only*. Sin trade, sin withdraw | fills, posiciones, órdenes, holdings |
| **OKX** | `api_key`, `api_secret`, `api_passphrase` | Solo *Read*. Sin trade, sin withdraw | fills, posiciones, órdenes, holdings |
| **OANDA** | `api_token` + el id de la cuenta | Un token de acceso personal. **OANDA no tiene token de solo lectura**: el mismo puede operar | fills, posiciones, órdenes, holdings |
| **Coinbase Advanced Trade** | `api_private_key` + el nombre completo de la clave | Clave CDP, solo **View**, creada como **Ed25519**. Sin Trade, sin Transfer | fills, órdenes, holdings |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` + el id de la cuenta | Sign-in OAuth que conceda `ReadAccount`, `MarketData`, `openid`, `offline_access`. **No `Trade`** | fills, posiciones, órdenes, holdings |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | Tu login más la AppKey que emite StoneX. **No existe credencial de solo lectura** | fills, posiciones, órdenes, holdings |
| **Capital.com** | `api_key`, `identifier`, `api_password` | Una clave API (el segundo factor debe estar activado) y su contraseña personalizada. **Sin nivel de solo lectura** | fills, posiciones, órdenes, holdings |
| **NinjaTrader** | `username`, `password`, `cid`, `sec` | Login de la plataforma más el par de claves de la API de desarrollador. **Sin clave de solo lectura** | fills, posiciones, órdenes, holdings |
| **Kraken** | `api_key`, `api_secret` | *Query Ledger & Trade History* y *Query Open Orders* | fills, órdenes, holdings |
| **Interactive Brokers (Flex)** | `flex_token` + un id de query Flex | Token del Flex Web Service: lee extractos, no puede operar | fills, posiciones, holdings |

Los secretos son de solo escritura: la app solo sabe *qué* nombres están definidos. Pueden escribirse, o enchufarse desde la [Bóveda](/es/config/settings#vault) para que una clave sirva a varias cuentas.

### Notas por broker

- **Alpaca**: el ajuste *Environment* es `live` o `paper`, y hay que indicarlo en lugar de adivinarlo, ya que ambos viven en hosts distintos con claves distintas. Alpaca no informa comisión en un fill, así que las operaciones importadas no llevan comisiones: correcto para acciones sin comisión, por debajo de la realidad para crypto y opciones, cuyas comisiones llegan como actividades de cuenta aparte.
- **Binance**, spot y futuros por igual, responde su historial de operaciones **un instrumento cada vez**, así que una consulta tiene que nombrar los instrumentos (`BTCUSDT`, `ETHEUR`). Cualquier otro broker de aquí responde la cuenta entera.
- **Tres plataformas crypto se detienen en 90 días.** Bitget, OKX y Binance futures sirven tres meses de fills por la API y no más; cualquier cosa anterior es una descarga desde su web. El modal de importación muestra el límite y rechaza un periodo que empiece antes, en lugar de devolver una respuesta a medias en silencio.
- **Una cuenta de derivados no es una cuenta spot.** Bitget, OKX y Binance futures pueden estar en corto, así que *Esta cuenta puede ir en corto* viene marcada por defecto ahí. En una cuenta Bitget configurada solo para el libro spot, desmárcala: una venta spot sin nada abierto es la venta de una moneda comprada antes, no un corto.
- **Un contrato se cuenta en contratos.** OKX informa los fills de derivados en contratos y publica cuánto vale uno (`ctVal × ctMult`), que se lee de su lista de instrumentos y se lleva como valor del punto de la operación. Los contratos de Bitget USDT-M y Binance USDⓈ-M se dimensionan en la moneda base, así que el suyo es uno. Los contratos Coin-M (inversos) no se leen en ningún sitio: se dimensionan en la moneda de cotización y su PnL no es una cantidad por un precio.
- **TradeStation es el único con OAuth.** No hay clave estática: un sign-in único en el navegador produce un refresh token, que la app intercambia por un token de acceso de 20 minutos sobre la marcha. Concede `ReadAccount`, `MarketData`, `openid` y `offline_access` en ese sign-in y deja fuera `Trade`; un token que pudiera operar sería un riesgo permanente sin motivo. Un fill aquí es un *tramo de orden*, ya que TradeStation publica órdenes cerradas y no ejecuciones, y la identidad sobre la que se deduplica una nueva sincronización es el id de la orden más el lugar del tramo en ella.
- **FOREX.com inicia sesión, no usa una clave.** Las credenciales son el usuario y la contraseña de la propia cuenta más la AppKey que emite StoneX una vez firmados los términos de su API, así que el mismo login puede operar: trátalo como un secreto de acceso total y cambia la contraseña al eliminar la cuenta. Una operación importada desde ahí lleva **sin comisión**, porque StoneX cobra el spread; si tu cuenta paga comisión en su lugar, las cifras de aquí quedarán por debajo de la realidad. El endpoint de historial de operaciones no admite fecha final ni cursor, así que un periodo amplio se recorre hacia delante en páginas de 200.
- **Capital.com responde un día cada vez.** Su registro de actividad limita el rango entre dos fechas a 24 horas, así que un año de trading cuesta una llamada por día; los periodos de más de 400 días se rechazan por su nombre en lugar de dejar que choquen con el limitador de tasa. Todos los instrumentos de la plataforma son **CFD**, así que un CFD de acción se archiva como derivado y no como la acción, que es lo que quiere un formulario fiscal. Un id de deal nombra una posición y no un fill, así que la identidad sobre la que se deduplica una nueva sincronización es el deal, su marca de tiempo y su dirección juntos.
- **NinjaTrader permite dos sesiones por login**, y una tercera cierra la más antigua: este connector mantiene una, y una aplicación de trading con sesión iniciada a su lado mantiene la otra. Su API de trading es la plataforma Tradovate que adquirió, por eso los errores dicen `tradovateapi`. Responde los fills que su sesión puede ver y no publica límite de profundidad, así que revisa la fila más antigua que devuelva la primera consulta antes de fiarte de ella para un año fiscal. El valor del punto de un futuro se lee del producto del contrato, nunca se asume a partir de la raíz.
- **OANDA no da token de solo lectura.** El token de acceso personal que lee esta cuenta también puede operarla, así que el formulario lo indica: trátalo como una credencial de acceso total y revócalo al eliminar la cuenta. Nada en la app lo usa jamás para escribir.
- **Interactive Brokers** no es una clave API en absoluto: lee un informe guardado mediante el Flex Web Service. La configuración, la regla del periodo y el ajuste de zona horaria tienen [su propia sección más abajo](#interactive-brokers-the-flex-web-service).
- Los límites de tasa son los del broker, y cada fila muestra el que se aplica. IBKR es el estricto: construye el extracto bajo demanda y rechaza una segunda petición mientras se genera una.

## Conectar una

1. **Añadir cuenta**, elige el broker y ponle nombre: el nombre es lo que muestran los selectores de los módulos, así que *Kraken principal* es mejor que *Kraken 2*.
2. Rellena las credenciales, o elígelas de la Bóveda. Una cuenta a la que le falta una se muestra como *incompleta* y todos los módulos la omiten hasta que la definas.
3. Rellena los ajustes no secretos que necesite el broker: un entorno (Alpaca, Binance futures, TradeStation, Capital.com, NinjaTrader), un id de cuenta (OANDA, TradeStation, Capital.com, FOREX.com, NinjaTrader), el nombre de la clave de Coinbase, el id de query y el offset de IBKR, o los libros de Bitget.
4. Elige los **módulos** a los que sirve, o *todos los módulos*.
5. **Probar conexión** llega al broker e informa de lo que respondió: el número y el estado de la cuenta, cuántos activos tienen saldo, o qué extracto devolvió el token Flex. Si algo va mal, el error nombra el ajuste que cambiar.

Las concesiones se aplican en el servidor: un módulo que pide una cuenta que nunca se le dio es rechazado.

## Qué te permite hacer

Cuatro destinos, una forma. Sea lo que sea que importes y donde sea que aterrice, una importación desde un broker corre sobre los mismos dos carriles que una importación desde archivo:

1. **Consultar, luego mirar.** La app pregunta al broker, pliega la respuesta en lo que almacena el módulo (posiciones para el journal, un balance para el portfolio, disposiciones cerradas para el formulario fiscal) y te lo muestra. En este paso no se escribe nada, así que una consulta que no te guste no cuesta nada.
2. **Confirmar y mantener el hilo.** Todo lo escrito lleva el id de esa importación, así que sigue siendo un solo objeto después: *Revertir* elimina exactamente lo que creó y nada más, *Conservar, dejar de rastrear* corta el vínculo y deja las filas en su sitio. Ambos viven en el historial de importaciones del módulo.

Los duplicados son tarea del carril, no tuya. Cada fila importada tiene una huella, así que volver a consultar un periodo que se solapa reconoce lo que ya está archivado, lo marca en la vista previa y lo escribe una sola vez.

### Importar operaciones al journal

**Journal → Importar → Consultar desde un broker**. Elige la cuenta, el periodo y el libro donde archivar, y los fills vuelven plegados en posiciones, con vista previa antes de escribir nada.

1. **La cuenta.** Solo se listan las concedidas al journal, y una incompleta lo indica. La última cuenta y periodo usados para un libro se recuerdan, así que la siguiente consulta son dos clics.
2. **El periodo**, por fecha o con los chips *7 / 30 / 90 / 365 días*. Léelo como la ventana en la que caen los *fills*, no la ventana en la que se cerraron las operaciones: una posición se pliega a partir de los fills dentro del periodo, así que empieza lo bastante pronto para captar la entrada.
3. **Los instrumentos.** Binance responde su historial un instrumento cada vez, así que allí los símbolos son obligatorios y *Sugerir* ofrece lo que contiene la cuenta. En todos los demás sitios el campo es un filtro: déjalo vacío para toda la cuenta.
4. **Cortos.** Una cuenta spot no puede estar en corto, así que una venta sin nada abierto se informa como error de fila nombrando la solución (ampliar el periodo) en lugar de convertirse en un corto fantasma. En una cuenta con margen, marca *Esta cuenta puede ir en corto*.
5. **Vista previa**, luego importar. Los contadores son fills, operaciones, de las cuales cerradas y abiertas, más lo que ya está en el libro y lo que no se pudo construir. Cada línea dice cuál es antes de confirmar.

- Mismo destino y mismo plegado que una importación desde archivo, sin el mapeo: una API responde campos tipados, así que las preguntas que plantea un CSV (qué columna es la fecha, si el decimal es una coma) no existen aquí.
- **Repetir es seguro.** La identidad de una posición es su fill de *apertura*, así que ampliar la ventana y volver a consultar actualiza lo que se ha cerrado desde entonces y deja el resto en paz, en lugar de archivar la misma operación dos veces.
- **Una actualización es solo mecánica.** Precios, cantidades, comisiones y fechas vienen del broker otra vez; tus notas, etiquetas, estrategia y campos de plantilla son tuyos y la sobreviven.

### Alinear un portfolio con lo que contiene la cuenta

**Portfolio → Desde un broker**. Lee un **balance**, no un historial de operaciones: la diferencia con tu libro mayor se muestra línea por línea, y eliges qué líneas alinear. Cada una que aceptas escribe la única operación que hace que el portfolio coincida.

- Un símbolo que el portfolio ya tiene se resuelve solo; cualquier otro se pregunta, porque "BTC" en un exchange es una cadena de texto y un activo aquí es una fuente de precio.
- **Nunca se inventa el coste base.** Interactive Brokers publica uno y se usa. Los exchanges crypto publican una cantidad y nada más, así que esas líneas lo indican y por defecto usan el precio de hoy, el único precio que nadie puede confundir con una afirmación sobre el pasado.
- **Tomar el precio del exchange.** En una línea sin coste base, un clic pregunta a la plataforma a cuánto cotiza el activo ahora mismo y rellena el precio. La línea nombra entonces el mercado que respondió (`BTCUSDT`, `XBT/USD`), y lo indica cuando ese mercado cotiza en algo distinto de la divisa propia del activo: un precio de Binance está en USDT, no en dólares. Un activo que la plataforma no cotiza se deja en paz en lugar de valorarse desde otro sitio, y escribes el precio tú.

Alpaca, Binance (spot y futuros), Bitget, Coinbase, Kraken, OKX, OANDA y TradeStation responden esa pregunta. Interactive Brokers Flex no: un extracto no es un feed de cotizaciones, FOREX.com cotiza un mercado por su id numérico en lugar de por el nombre que lleva una línea del portfolio, y NinjaTrader sirve precios mediante un derecho de datos de mercado aparte. Capital.com responde con el punto medio de los dos lados en los que opera.

### Dibujar tu libro en el gráfico

**Gráfico → Broker book**. Sincroniza una cuenta y sus posiciones y órdenes activas se dibujan como niveles de precio en el gráfico del instrumento correspondiente, coste medio para una posición, límite y stop para una orden. La coincidencia se hace por ticker, sin contar la puntuación. Una posición sin coste medio no recibe línea y se cuenta como tal.

### Leer un año fiscal

**Impuestos → Desde un broker**. Consulta los fills, los pliega en posiciones cerradas y totaliza lo realizado dentro del año fiscal, repartido entre las líneas de capital, derivados y crypto del formulario. **No escribe nada**: aplicas las cifras al formulario y guardas el escenario tú mismo.

- **La ventana no es el año fiscal.** Lo que vendiste en marzo se compró antes, y sin esa compra no hay coste base: mueve la fecha de inicio hacia atrás lo suficiente para cubrirla. Una disposición cuya compra falta se informa, nunca se valora contra nada.
- Cada posición cerrada se convierte al tipo de **su propia fecha de salida**. Una que no tiene tipo se lista y se deja fuera de los totales en lugar de sumarse en la divisa equivocada.

## Interactive Brokers: el Flex Web Service {#interactive-brokers-the-flex-web-service}

IBKR es el único broker de aquí que no se lee mediante una API de trading. Se lee mediante **Flex**, el servicio de informes de Account Management: guardas una query que describe lo que quieres en un extracto, y la app obtiene ese extracto por HTTPS con un token.

### Por qué Flex y no TWS

El [connector de datos de mercado](/es/config/connectors#interactive-brokers) habla el socket de TWS con un Gateway que ejecutas tú mismo. Ese socket es la herramienta adecuada para el presente, y la equivocada para el historial: responde posiciones abiertas y los fills de la **sesión actual**, así que *importar mis operaciones de marzo* no tiene forma de socket en absoluto. Flex sirve un periodo, que es exactamente la pregunta que plantea una importación.

La diferencia práctica:

| | Flex Web Service | Socket de TWS / IB Gateway |
|---|---|---|
| **Qué ejecutas** | nada, es una llamada HTTPS | Gateway o TWS, con sesión iniciada, en la máquina |
| **Historial** | el periodo de la query, hasta un año atrás | solo la sesión actual |
| **Credencial** | un token que lee informes | tu sesión en vivo, capaz de operar |
| **Se usa aquí para** | importación al journal, holdings del portfolio, año fiscal, posiciones del gráfico | precios, gráficos, barras en vivo |

### Por qué es la vía segura

- **El token no puede operar.** Se emite para el Flex Web Service y ese servicio sirve extractos. No hay detrás un endpoint de órdenes que olvidar desactivar, ni casilla de permisos que equivocar. Compáralo con una clave API de un exchange, donde el solo lectura es una casilla que tienes que acordarte de marcar.
- **No queda nada escuchando.** Ningún Gateway en marcha, ningún puerto API abierto, ninguna IP de confianza que declarar, nada esperando en tu máquina mientras la app está inactiva.
- **Caduca solo.** IBKR da un tiempo de vida a un token y envía un recordatorio por correo antes de que expire. Un token olvidado deja de funcionar en lugar de seguir válido para siempre.
- **La query es la valla.** Un token solo puede devolver lo que describen las queries que guardaste. Mantén la query en operaciones y posiciones abiertas y eso es todo lo que la app podrá ver jamás, pida lo que pida.
- Como toda credencial de aquí, el token es **de solo escritura en la app**: puede escribirse o enchufarse desde la [Bóveda](/es/config/settings#vault), y nunca se vuelve a mostrar.

### Cómo se ejecuta realmente una consulta

1. La app llama a `SendRequest` con tu token y el id de query. IBKR responde con un código de referencia y empieza a **construir** el extracto.
2. Luego consulta `GetStatement` con ese código hasta que llega el XML, lo que normalmente son unos segundos y puede ser más para una query amplia. Un extracto que aún se genera es la respuesta esperada a los primeros intentos, no un error.
3. El extracto se analiza en fills (filas `Trade` a nivel de ejecución) y holdings (`Open Positions`), y la app filtra esos datos por el periodo que elegiste.

Si IBKR sigue generando tras un minuto, la app lo dice en lugar de colgarse: acota el rango de fechas de la query, o inténtalo de nuevo en un momento.

### Configurarlo

1. **El token**: Account Management → Settings → **Flex Web Service**. Genera uno, cópialo una sola vez, anota la fecha de caducidad.
2. **La query**: Account Management → Performance & Reports → **Flex Queries** → nueva query *Activity*. Incluye:
   - **Trades**, nivel de detalle **Execution**, para el journal y el formulario fiscal;
   - **Open Positions**, para la alineación del portfolio y la superposición del gráfico.

   Guárdala y anota el **id de query**, el número que aparece junto a su nombre.
3. En OpenTraderWorld: añade la cuenta, pega el token, rellena **Flex query id** y **Statement time offset**, y luego **Probar conexión**. Informa del número de cuenta, el periodo que cubre el extracto y cuántas filas de operaciones contiene, que es la forma más rápida de ver que a la query le falta una sección.

::: tip Dos ajustes que deciden si la importación es correcta
**El periodo es el de la query, no el tuyo.** Una query Flex lleva su propio rango de fechas (*Last 365 Calendar Days*, *Year to Date*, una ventana personalizada) y el web service no admite fechas en absoluto. Las fechas que eliges en la app **filtran** lo que devolvió el extracto, así que una query configurada en *Last 30 days* nunca dará marzo por muy atrás que pidas, e IBKR no sirve más de un año. Configura la query amplia y filtra en la app.

**Un extracto Flex nunca nombra su zona horaria.** Marca con la zona propia de la query y no dice cuál es, así que define *Statement time offset* con esa zona en minutos (`-300` Nueva York en invierno, `60` París) o cada fill caerá en la hora equivocada, y las operaciones intradía en el día equivocado.
:::

### Lo que Flex no hace

- **Sin órdenes activas**, así que la superposición del gráfico dibuja las posiciones de IBKR a su coste medio y ningún nivel de orden.
- **Sin cotizaciones.** Un extracto no es un feed de precios: el botón *tomar el precio del exchange* del portfolio lo ofrecen los brokers con API, no este. IBKR es el único de los cinco que publica un **coste base**, que es el número que importa para un libro mayor.
- **Un extracto cada vez.** IBKR lo construye bajo demanda y rechaza una segunda petición mientras se genera uno, así que las consultas seguidas sobre la misma query se esperan entre sí.

## Concesiones a módulos

| Módulo | Qué lee |
|---|---|
| **Trading Journal** | los fills del periodo, para la importación |
| **Portfolios** | lo que contiene la cuenta |
| **Visualización** | posiciones y órdenes activas, para la superposición del gráfico |
| **Calculadora de Impuestos** | los fills de un año fiscal |

Marcar todos los módulos vuelve al comodín *todos los módulos*, que también cubre los módulos añadidos en versiones futuras.

## Límites

- **Un ticker nunca se adivina.** Un símbolo que la app no puede resolver es un error que nombra la solución, no una coincidencia aproximada.
- Lo que un broker no publica queda vacío en lugar de plausible: sin coste base inventado, sin comisión inventada, sin lado inventado.
- Eliminar una cuenta elimina sus credenciales. Lo que ya importó se queda.
- Las cuentas de broker están desactivadas en el [modo demo](/es/guide/demo).
