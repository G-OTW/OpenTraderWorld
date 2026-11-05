# Fundamentales

Un solo lugar para leer la economía y una empresa desde las fuentes que publican las cifras: series macro con gráficos, estados financieros de empresas, filings de la SEC, transcripciones de earnings calls, holdings de ETF, un calendario de mercado y datos alternativos. Todo se almacena en tu propia base de datos, así que los gráficos, el [Agent](/es/modules/agent) y otros módulos lo leen sin volver a preguntar al proveedor.

Fundamentales no tiene ajustes de proveedor propios. Cada fuente es un **[connector de datos](/es/config/connectors)** concedido al módulo: el icono de enchufe en la cabecera de la página abre la pantalla compartida de connectors. Muchas fuentes son organismos públicos **sin clave** (SEC EDGAR, el Tesoro de EE. UU., el BCE, Eurostat, el BIS, la OCDE, el FMI, el Banco Mundial, la CFTC, FINRA, USAspending); el resto usan una clave gratuita o de pago que aportas tú. No se obtiene nada hasta que añades un connector y lo concedes a Fundamentales.

## Páginas

| Página | Qué muestra |
|---|---|
| **Macro** | Tus series por categoría (crecimiento, inflación, empleo, tipos, dinero, encuestas, vivienda, energía, fiscal, posicionamiento), hasta cuatro en un gráfico, la curva de rendimientos del Tesoro y los tipos oficiales de los bancos centrales. |
| **Empresa** | Perfil y métricas clave, estados financieros, estimaciones, resultados, segmentos, dividendos y recompras, accionariado, peers, ESG y retribución, filings y transcripciones. |
| **ETF** | Perfil, principales holdings, exposición por sector y país. |
| **Eventos** | Próximos resultados, IPO, operaciones corporativas y decisiones de bancos centrales. |
| **Documentos** | Cada filing y transcripción almacenados, con búsqueda de texto completo y el lector de transcripciones. |
| **Datos alternativos** | Operaciones del Congreso, gasto en lobbying, contratos federales y patentes concedidas. |
| **Biblioteca** | Pestañas para las series y empresas que conservas (filtrables), la prioridad de fuentes y qué proveedor sirve qué familia de datos. |

**Personalizar** (arriba a la derecha) define la densidad, qué secciones se muestran y en qué orden, página por página.

## Series macro {#macro}

**Añadir series** busca en el catálogo de un proveedor o toma el código propio del proveedor (`CPIAUCSL` en FRED, `HICP/M.U2.N.000000.4D0.ANR` en el BCE). Un código se comprueba contra el proveedor antes de almacenar nada: un código desconocido es un error que lo nombra, nunca una serie vacía. **Añadir un conjunto inicial** añade con un clic una primera selección de series de EE. UU. y la zona euro.

| Proveedor | Clave | Qué cubre |
|---|---|---|
| FRED | gratis | La mayoría de series de EE. UU. (también refleja BLS, BEA y Census) |
| Tesoro de EE. UU. | ninguna | Curva de rendimientos par diaria, deuda pública total |
| BCE, Eurostat | ninguna | Inflación, tipos, dinero, PIB y desempleo de la zona euro |
| BIS | ninguna | Tipos oficiales de bancos centrales, tipos de cambio efectivos |
| OCDE, FMI, Banco Mundial | ninguna | Indicadores adelantados, World Economic Outlook, datos anuales por país |
| BLS, BEA, EIA, US Census | gratis | Detalle de EE. UU. cuando FRED se retrasa o no tiene una serie |
| CFTC | ninguna | Commitments of Traders, posicionamiento neto no comercial |

Una fila es un **periodo**: una observación se almacena contra el inicio del periodo que cubre. El gráfico calcula las transformaciones al leer (nivel, interanual, variación del periodo, diferencia, índice 100), así que nunca se almacena nada derivado. El interanual compara cada valor con el fechado un año antes; un periodo ausente muestra un hueco en lugar de una comparación con el mes equivocado. El sombreado de recesiones sigue las fechas del NBER.

## Empresas {#company}

Empresa, ETF y Datos alternativos comparten un **selector de símbolos**: primero tus favoritos, luego los 15 abiertos más recientemente. Su búsqueda cubre todos los símbolos almacenados, más coincidencias de EDGAR para empresas.

Escribe un ticker. Se resuelve en la lista de tickers de SEC EDGAR; un ticker que EDGAR no conoce es un error, nunca una mejor suposición. Abrir una empresa la almacena y obtiene, en segundo plano:

- **Estados financieros** a partir de los company facts XBRL, anuales y trimestrales. Los cuartos trimestres y las líneas de flujo de caja acumuladas del año se derivan por diferencia; cada línea conserva la etiqueta bajo la que se informó.
- **Filings** (10-K, 10-Q, 8-K, proxies...) con un enlace a la fuente.
- **Operaciones de insiders** analizadas del Form 4.

Las demás pestañas leen los agregadores que conectes, la mejor fuente primero. Un proveedor cuyo plan deja fuera un dataset (una clave gratuita de FMP y el historial de resultados, por ejemplo) pasa al siguiente, y un connector concedido al módulo sin su clave se omite. Para el precio, gana el historial más largo (los planes gratuitos suelen detenerse en uno o dos años):

| Datos | Proveedores |
|---|---|
| Estimaciones, precios objetivo, acciones de rating | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Resultados (EPS estimado y real, próxima fecha) | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Segmentos | Financial Modeling Prep |
| Dividendos y splits | EODHD, Massive, Financial Modeling Prep, Alpha Vantage |
| Holders 13F | Financial Modeling Prep |
| Posiciones cortas | FINRA, Massive |
| Peers | Financial Modeling Prep, Finnhub |
| ESG y retribución de ejecutivos | Financial Modeling Prep, Finnhub |
| Transcripciones | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Precio y ratios de mercado | cualquier connector de datos de mercado con barras diarias de acciones |

**Biblioteca → Prioridad de fuentes** lista cada dataset con más de una fuente (transcripciones incluidas) en el orden en que se prueban sus proveedores. Elige una posición junto a un proveedor para moverlo allí; **Orden predeterminado** restaura el orden de la app. El precio no tiene orden: gana el historial más largo.

Cada pestaña dice qué proveedor respondió y cuándo, o el error que nombra la solución (normalmente un connector que añadir o un plan que no incluye el dataset). Una respuesta se conserva y reutiliza hasta que caduca (unas horas para el calendario, un día para las estimaciones, una semana para los holders); **Actualizar** vuelve a preguntar ahora.

Abrir una página no gasta tu cuota en un proveedor que acaba de negarse: uno que rechazó el dataset (plan, símbolo, límite de tasa) se deja en paz un tiempo, desde unos minutos tras un error de red hasta una semana tras un rechazo de plan, y lo mismo uno cuya cuota, declarada en su connector, está agotada. La pestaña lo indica y cuándo es el siguiente intento automático; **Actualizar** pregunta a todos los proveedores a la vez.

**Seguir** una empresa hace que se actualice a diario y que se te avise de sus nuevos filings.

### Transcripciones {#transcripts}

La pestaña Transcripciones lista las llamadas que un proveedor tiene de la empresa; el texto de una transcripción se obtiene la primera vez que la abres, dividido en turnos de hablante, con las declaraciones preparadas separadas del Q&A, y con búsqueda junto a los demás documentos.

## Datos alternativos {#alt}

La empresa es la que está abierta en Empresa; un ticker escrito aquí se abre primero.

| Pestaña | Proveedores |
|---|---|
| Operaciones del Congreso | Quiver Quant (lo último de todos los miembros, o de una empresa), Finnhub premium (por empresa) |
| Lobbying | LDA.gov, Quiver Quant |
| Contratos del gobierno | USAspending, Quiver Quant |
| Patentes | USPTO Open Data Portal (clave gratuita), Quiver Quant |

Las fuentes públicas conocen a una empresa por su **nombre registrado**, no por su ticker. LDA.gov, USAspending y la USPTO se consultan con el nombre que almacena EDGAR, y un registro solo cuenta cuando su nombre es el mismo una vez eliminados la puntuación y el sufijo legal (`Lockheed Martin Corp` coincide con `LOCKHEED MARTIN CORPORATION`, nunca con `Lockheed Martin Aculight`). Cada pestaña muestra el nombre con el que coincidió. Para los contratos se usa el beneficiario matriz, así que cuentan las filiales registradas bajo él y no una registrada por separado (Amazon Web Services bajo Amazon).

LDA.gov requiere una clave gratuita (regístrate en lda.gov). Su firewall rechaza las redes de fuera de EE. UU. (un HTTP 403 que lo nombra): accede desde una conexión de EE. UU., o usa Quiver Quant. Un informe cuenta una vez: una enmienda sustituye al original, y los registros de lobistas, que no llevan gasto, se dejan fuera. Una empresa que hace lobbying mediante una filial con otro nombre (JPMorgan Chase Holdings para JPMorgan Chase) solo muestra los informes presentados bajo su propio nombre.

## Cobertura de proveedores {#coverage}

Qué proveedor puede servir qué datos, como en **Biblioteca → Cobertura de proveedores**. Una familia servida por varios proveedores se prueba en el orden de **Biblioteca → Prioridad de fuentes**. El precio y los ratios de mercado vienen de cualquier connector de datos de mercado con barras diarias de acciones (Alpha Vantage, EODHD, Massive, Yahoo, IBKR...), no listados aquí.

| Proveedor | Clave | Macro | COT | Estados financieros | Filings | Insiders | Estimaciones | Resultados | Segmentos | Dividendos | Holders | Posiciones cortas | Peers | ESG | ETF | Calendario | Transcripciones | Datos alternativos |
|---|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| SEC EDGAR | ninguna |  |  | ✓ | ✓ | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |
| FRED (St. Louis Fed) | gratis | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Treasury | ninguna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| ECB Data Portal | ninguna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| Eurostat | ninguna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BIS | ninguna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| OECD | ninguna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| IMF | ninguna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| World Bank | ninguna | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BLS | gratis | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BEA | gratis | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| EIA | gratis | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Census | gratis | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| CFTC | ninguna |  | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| FINRA | ninguna |  |  |  |  |  |  |  |  |  |  | ✓ |  |  |  |  |  |  |
| Financial Modeling Prep | plan gratuito |  |  |  |  |  | ✓ | ✓ | ✓ | ✓ | ✓ |  | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Finnhub | plan gratuito |  |  |  |  |  | ✓ | ✓ |  |  |  |  | ✓ | ✓ |  | ✓ | ✓ | ✓¹ |
| Alpha Vantage | plan gratuito |  |  |  |  |  | ✓ | ✓ |  | ✓ |  |  |  |  | ✓ | ✓ | ✓ |  |
| EODHD | plan gratuito |  |  |  |  |  |  |  |  | ✓ |  |  |  |  | ✓ | ✓ |  |  |
| Massive (Polygon.io) | plan gratuito |  |  |  |  |  |  |  |  | ✓ |  | ✓ |  |  |  |  |  |  |
| USAspending | ninguna |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| LDA.gov (lobbying) | gratis |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| USPTO Open Data Portal | gratis |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| Quiver Quant | de pago |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |

¹ Finnhub sirve las operaciones del Congreso solo en un plan premium.

Orientativo: los planes cambian, y un plan gratuito puede dejar fuera una familia (Financial Modeling Prep gratuito: sin holders 13F, transcripciones ni holdings de ETF; Finnhub gratuito: sin ESG ni operaciones del Congreso; Alpha Vantage gratuito: 25 peticiones al día). Consulta la página del propio proveedor antes de pagar.

## Actualización automática y notificaciones {#refresh}

Las series almacenadas se actualizan cuando su frecuencia indica que puede haber un valor nuevo: una serie diaria dos veces al día, una semanal o mensual a diario, una trimestral cada tres días, una anual cada semana. Las empresas seguidas se actualizan desde EDGAR una vez al día. Una fuente sin connector concedido se omite.

Una serie con un periodo nuevo y una empresa seguida con un filing nuevo (salvo los formularios de insiders) generan una notificación, enviada a los [canales](/es/config/settings#notifications) concedidos a Fundamentales (la campana de la cabecera de la página).

## Widgets del dashboard {#dashboard}

El [Dashboard](/es/modules/dashboard) ofrece series y tableros macro, instantáneas de empresas, historial de estados financieros, empresas, filings, próximos resultados y comparaciones de valoración. Las tarjetas muestran divisa, base de reporte y fechas; su actualización lee solo datos almacenados. Próximos resultados usa la instantánea de resultados almacenada de una empresa o, cuando no tiene fecha futura, el calendario de mercado almacenado. La valoración usa empresas almacenadas seleccionadas a mano o los peers almacenados de su pestaña Peers. Las estimaciones y ratios ausentes siguen sin estar disponibles en lugar de inferirse.

## Búsqueda y agents {#search}

La búsqueda de la barra superior, con los títulos de contenido activados, encuentra empresas, series y títulos de documentos almacenados.

Los agents acceden a lo almacenado, solo lectura, mediante la [pasarela MCP](/es/config/ai-agents) cuando a un token se le concede **Fundamentales**: series y observaciones, empresas, estados financieros, filings, transcripciones, operaciones de insiders y cada dataset almacenado. Consultar un proveedor, actualizar y añadir series quedan fuera: gastan tu cuota de proveedor.
