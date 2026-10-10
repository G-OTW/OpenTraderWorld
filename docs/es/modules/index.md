# Resumen de módulos

OpenTraderWorld es un conjunto de **módulos**, paquetes de funciones que activas individualmente en **Ajustes → Módulos**. Todo viene con la app; instalar un módulo hace que aparezca en el selector de módulos (arriba a la izquierda) y en el dashboard. Desvincula un módulo para ocultarlo de nuevo (sus datos se conservan salvo que también los elimines).

El [dashboard, la búsqueda y las notificaciones](/es/modules/dashboard) están por encima de todos ellos y siempre están ahí.

## Dependencias

Algunos módulos se apoyan en el catálogo de datasets de **Datos históricos** y necesitan que esté instalado:

```
Historical Data ──▶ Historical Data Visualization
                ──▶ Backtest
                ──▶ Quant Tools
```

Todo lo demás es independiente, aunque algunos módulos se integran cuando ambos están instalados (p. ej. la Calculadora de Impuestos puede importar el PnL del Trading Journal; MyWealth puede importar los holdings del Seguimiento de Portfolio; el Calendario puede mostrar Tareas, Objetivos y Recordatorios).

Datos históricos, Visualización, Watchlists, Fundamentales y el Trading Journal comparten además una lista de **[connectors de datos](/es/config/connectors)**: una cuenta de proveedor se crea una vez y se concede a los módulos que pueden usarla.

## Todos los módulos

### Trading

| Módulo | Qué hace |
|---|---|
| [Trading Journal](/es/modules/journal) | Registro de operaciones con plantillas, tablas de comisiones, FX multidivisa y estadísticas de rendimiento. |
| [Rutinas de trading](/es/modules/productivity#routines) | Listas de verificación de sesión recurrentes: preparación previa al mercado, disciplina durante la sesión, revisión posterior al mercado. |
| [Mentalidad](/es/modules/productivity#mindset) | Check-ins diarios de ánimo y disciplina con tendencias. |

### Datos de mercado y análisis

| Módulo | Qué hace |
|---|---|
| [Datos históricos](/es/modules/market-data#histdata) | Descarga historial OHLCV de varios proveedores a datasets locales. |
| [Visualización de datos históricos](/es/modules/market-data#histviz) | Un espacio de trabajo de gráficos de velas/OHLC/línea/Renko con indicadores, dibujos, comparaciones y alertas vigiladas por el servidor, en vivo o bajo demanda, sobre cualquier instrumento que sirva un connector. |
| [Backtest](/es/modules/market-data#backtest) | Backtester de estrategias basadas en reglas con sizing, costes y estadísticas completas. |
| [Herramientas Cuant](/es/modules/market-data#quant) | Riesgo, estadísticas, volatilidad y regímenes de un dataset; pares, cestas y regresión de factores; tests de overfitting sobre backtests guardados; sizing, calculadoras, curvas de futuros y superficies de volatilidad de opciones. |
| [Fundamentales](/es/modules/fundamentals) | Series macro, estados financieros de empresas, filings de la SEC, transcripciones, ETF, un calendario de mercado y datos alternativos de fuentes primarias y de los agregadores que conectes. |

### Portfolios y dinero

| Módulo | Qué hace |
|---|---|
| [Watchlists](/es/modules/portfolio#watchlists) | Watchlists de símbolos con precios en vivo, variaciones diarias, minigráficos y notas. |
| [Seguimiento de Portfolio](/es/modules/portfolio#portfolios) | Valor en vivo, libro mayor de efectivo e ingresos, rendimiento frente al riesgo asumido, desviación de la asignación y stress testing. |
| [MyWealth](/es/modules/portfolio#wealth) | Patrimonio neto de todo lo que posees y debes, con los portfolios leídos en vivo en lugar de copiados. |
| [Carteras de gestores](/es/modules/portfolio#mportfolios) | Posiciones 13F de superinversores, explorables y con instantáneas. |
| [Calculadora de Impuestos](/es/modules/portfolio#taxcalc) | Estimaciones fiscales de trading e inversión a partir de plantillas por país. |
| [Suscripciones](/es/modules/portfolio#subscriptions) | Suscripciones recurrentes y resumen del gasto. |

### Noticias e investigación

| Módulo | Qué hace |
|---|---|
| [Noticias](/es/modules/news-research#news) | Agregador de noticias RSS y JSON-API con dashboards de polling. |
| [Buzón](/es/modules/news-research#mailbox) | Newsletters, noticias de mercado y correo del broker leídos desde tu propio buzón IMAP, sin rastreadores. |
| [Calendario Económico](/es/modules/news-research#economics) | Próximos eventos macro. |
| [FinanceDatabase](/es/modules/news-research#findb) | Busca más de 300.000 instrumentos en local; organiza favoritos en carpetas. |
| [Recursos](/es/modules/news-research#resources) | Biblioteca de marcadores para libros, enlaces y referencias. |
| [Docs de la comunidad](/es/modules/news-research#community-docs) | Guías escritas por la comunidad, sincronizadas y legibles sin conexión. |

### Notas y organización

| Módulo | Qué hace |
|---|---|
| [Editor](/es/modules/productivity#editor) | Editor de documentos enriquecido con carpetas y bases de datos de tipo tabla/kanban/galería. |
| [Tareas](/es/modules/productivity#todos) | Lista de tareas con fechas límite y categorías. |
| [Objetivos](/es/modules/productivity#goals) | Objetivos con seguimiento de métricas y fechas límite. |
| [Calendario](/es/modules/productivity#calendar) | Calendario de eventos personal; superpone recordatorios, tareas y objetivos. |
| [RemindMe](/es/modules/productivity#remindme) | Recordatorios con notificaciones en la app y canales de email/Telegram/Slack/Discord. |
| [Control de Tiempo](/es/modules/productivity#time) | Temporizadores de proyectos con presupuestos y valor por tarifa horaria. |
| [Biblioteca de prompts](/es/modules/productivity#prompt-store) | Biblioteca con búsqueda de prompts de IA reutilizables, etiquetados, valorados y versionados. |
| [Webhooks](/es/modules/productivity#webhooks) | URLs entrantes privadas que convierten alertas externas en notificaciones. |
| [Automator](/es/modules/automator) | Workflows sobre tu propia API y el mundo exterior, a mano o programados. |

### IA

| Módulo | Qué hace |
|---|---|
| [Agent](/es/modules/agent) | Asistente de chat con IA integrado (trae tu propio proveedor) que también puede actuar sobre tus datos mediante MCP, con memoria, skills y servidores MCP externos. |
