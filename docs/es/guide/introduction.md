# ¿Qué es OpenTraderWorld?

> Sitio web del proyecto: **[opentraderworld.com](https://opentraderworld.com)**: recorrido por los módulos,
> [demo en vivo](https://demo.opentraderworld.com), [guías de la comunidad](https://opentraderworld.com/docs)
> y [votación de la hoja de ruta](https://opentraderworld.com/suggestions).

OpenTraderWorld es una **plataforma web autoalojada para traders e inversores**. La instalas una sola vez con Docker en tu propio ordenador o servidor, la abres en un navegador y obtienes un espacio de trabajo privado formado por módulos: un trading journal, datos históricos de mercado con gráficos y backtesting, seguimiento de portfolios y patrimonio neto, un agregador de noticias, notas, listas de verificación y más.

**Gratis para todos, uso personal o profesional. Código disponible (FSL-1.1-MIT).** Lo único que no puedes hacer es revenderla u ofrecerla como servicio de pago. El principio rector: *sé rentable antes de gastar un céntimo.*

## ¿Por qué autoalojada?

- **Tus datos son tuyos.** Operaciones, portfolios, notas y entradas del journal viven en una base de datos PostgreSQL en tu máquina, no en el servidor de otra persona.
- **Privada por defecto.** Tras la instalación, la app escucha solo en `localhost`. Exponerla a tu LAN o a internet es una decisión explícita que tomas en [Ajustes → Red](/es/config/network).
- **Sin suscripción.** Las herramientas básicas no cuestan nada. Algunos módulos pueden usar opcionalmente proveedores de datos externos (muchos con plan gratuito), y tú aportas tus propias claves API.

## Cómo funciona

Una sola pila de `docker compose`, cuatro servicios:

| Servicio | Función |
|---|---|
| **core** | Servidor API en Rust (Axum): toda la lógica de negocio, planificador y tareas en segundo plano |
| **postgres** | PostgreSQL: el único lugar donde viven tus datos |
| **frontend** | Aplicación de una sola página en SvelteKit, compilada una vez al desplegar |
| **caddy** | Proxy inverso: sirve la app, hace de proxy de `/api` y gestiona los certificados HTTPS |

La app es **monousuario**: una cuenta de administrador, creada en la instalación. No hay modo multiinquilino, ni compartición, ni gestión de usuarios que configurar.

Docker es actualmente el **único despliegue admitido**: mantiene la instalación poco intrusiva y rápida de reconstruir ([por qué, y cómo obtener Docker](/es/guide/docker)). Una instalación nativa es posible, pero no se recomienda.

## Los módulos

Los módulos son paquetes de funciones que instalas o desvinculas desde **Ajustes → Módulos**. Todo viene incluido en la app, e instalar un módulo simplemente lo activa. Los más destacados:

- **[Trading Journal](/es/modules/journal)**: registra operaciones con plantillas, tablas de comisiones, PnL multidivisa y estadísticas de rendimiento completas.
- **[Datos de mercado y backtesting](/es/modules/market-data)**: descarga historial OHLCV de varios proveedores, representa cualquier instrumento en vivo o bajo demanda con indicadores, haz backtest de estrategias basadas en reglas y ejecuta analítica cuantitativa sobre datasets, backtests guardados, curvas de futuros y cadenas de opciones.
- **[Portfolios y patrimonio](/es/modules/portfolio)**: seguimiento de portfolios en vivo, historial de patrimonio neto, posiciones 13F de superinversores, estimaciones fiscales.
- **[Noticias e investigación](/es/modules/news-research)**: dashboards de noticias RSS/API, calendario económico, un catálogo de búsqueda de 300 mil instrumentos.
- **[Notas y organización](/es/modules/productivity)**: editor de texto enriquecido con bases de datos, tareas, objetivos, calendario, recordatorios, rutinas de trading y check-ins de mentalidad.
- **[AI Agent](/es/modules/agent)**: asistente de chat integrado (trae tu propio proveedor) que puede actuar sobre tus datos mediante MCP, con memoria, skills y servidores MCP externos.

Consulta la [lista completa de módulos](/es/modules/).

## Próximos pasos

1. [Instala OpenTraderWorld](/es/guide/install): unos 5 minutos con Docker.
2. [Da tus primeros pasos](/es/guide/first-steps): inicia sesión, elige valores predeterminados, instala módulos.
3. [Configura el acceso a la red](/es/config/network): si quieres acceder desde otros dispositivos.

¿No estás listo para instalar? Prueba la [demo en vivo](https://demo.opentraderworld.com), una instancia compartida
con datos de ejemplo, restablecida cada 15 minutos. [Qué es el modo demo](/es/guide/demo) y qué bloquea.
