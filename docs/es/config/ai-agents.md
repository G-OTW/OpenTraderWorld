# AI agents (MCP)

OpenTraderWorld incluye un **servidor MCP** integrado para que los AI agents (cualquier cliente compatible con [MCP](https://modelcontextprotocol.io)) puedan leer y actualizar tus módulos mediante una pasarela controlada. Un agent puede registrar operaciones del journal por ti, resumir tus feeds de noticias, añadir tareas, consultar los resultados de tus backtests, etcétera.

**Está desactivado por defecto.** Nada escucha a los agents hasta que lo actives.

::: tip ¿Buscas el asistente de chat de la app?
Esta página trata de agents **externos** que se conectan *a* OpenTraderWorld. Si quieres el asistente de chat integrado que vive dentro de la app (trae tu propio proveedor), consulta el [módulo Agent](/es/modules/agent): puede *usar* esta misma pasarela para acceder a tus datos.
:::

## Modelo de seguridad {#security-model}

Varias capas, y todas deben superarse:

1. **Interruptor global**: el endpoint MCP está desactivado hasta que lo actives en **Ajustes → MCP**. Puedes preparar tokens mientras está apagado; toda petición de un agent se rechaza hasta que se active.
2. **Tokens Bearer**: uno por agent o caso de uso. Los tokens se almacenan **con hash** y se muestran solo **una vez** al crearlos; los intentos fallidos se limitan. Revoca un token cuando quieras.
3. **Permisos de módulo por token**: cada token concede *sin acceso*, *lectura*, *lectura + escritura* o *completo (lectura + escritura + borrado)* **por módulo**. Los agents solo descubren los módulos que concediste.
4. **Lista de permitidos estricta**: las operaciones de cuenta, red, secretos, almacenamiento de archivos y borrado de datos **nunca se exponen** a los agents, sean cuales sean los permisos.

::: tip La lista de permitidos es deliberada, no automática
Un endpoint solo es accesible para los agents porque alguien lo añadió al catálogo a mano. Un módulo nuevo, o una ruta nueva en uno existente, es **invisible para todos los agents** hasta que exista esa entrada, de modo que la pasarela nunca puede ampliarse por accidente a medida que la app crece. La gestión de personas y skills se deja fuera a propósito: ningún agent, ni ningún contenido que un agent lea, puede editar una persona ni ampliar su estantería de skills.
:::

::: tip Las versiones funcionan como commits
Con el versionado activado en **Ajustes → Versiones**, un agent con permiso sobre **Editor** o **Backtest** puede guardar una versión de un documento o estrategia tras actualizarlo, con una nota que diga qué cambió, y listar, leer, restaurar o eliminar versiones. Puede activar el versionado por archivo o estrategia, pero no los interruptores globales de Ajustes.
:::

::: warning El Automator concede escribir, no armar
Conceder **Automator** permite a un agent leer tus workflows, crear uno, escribir su grafo y probarlo. **No** le permite ponerlo en servicio: un grafo que un agent guarda queda como borrador que adoptas desde el editor, no puede asociar el token de acceso de un workflow y no puede ejecutar un workflow ni tocar una programación. Un workflow se ejecuta con su propio token y no con el del llamante, así que escribir un grafo y armarlo son dos concesiones distintas. Consulta [la página del módulo](/es/modules/automator#letting-an-agent-build-a-workflow).
:::

## Activar y crear un token

1. Ve a **Ajustes → MCP** y actívalo.
2. **Nuevo token**: ponle el nombre del cliente (p. ej. `My Agent`), define los permisos por módulo (o usa *Toda lectura* / *Toda lectura+escritura* / *Todo completo* como punto de partida).
3. **Copia el token de inmediato**: se muestra una sola vez.

**Permitir acceso externo** es una casilla aparte en el mismo diálogo. No concede nada adicional: solo permite que ese token respalde un vínculo de chat en [Control externo](/es/config/external-control), donde un mensaje de Telegram, Slack o Discord se ejecuta con esos mismos niveles por módulo.

El diálogo de creación también muestra un **fragmento de configuración listo para pegar**, con una pestaña por familia de clientes: el endpoint y la cabecera en bruto, un bloque JSON `mcpServers` (Cursor, Cline, Windsurf, VS Code…) y una línea de comando `claude mcp add`. Los mismos fragmentos siguen disponibles bajo la tabla de tokens con `<TOKEN>` como marcador, para configurar una segunda máquina más tarde.

## Conectar un cliente

El endpoint habla **MCP sobre Streamable HTTP** en:

```
POST http://<your-host>/api/mcp
Authorization: Bearer <TOKEN>
```

Cualquier cliente compatible funciona. Ejemplo de configuración MCP:

```json
{
  "mcpServers": {
    "opentraderworld": {
      "type": "http",
      "url": "http://localhost:5454/api/mcp",
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

Sustituye la URL por tu dominio si usas un modo LAN/HTTPS.

::: tip Instalaciones solo localhost
Si la app solo es accesible en `localhost` (el modo de red por defecto), los agents deben ejecutarse **en la misma máquina**.
:::

## Conectar con OAuth (claude.ai, ChatGPT)

Algunos clientes no pueden guardar un token fijo: los connectors de claude.ai y ChatGPT solo inician sesión con OAuth. Para ellos, activa **Sign-in OAuth** al final de **Ajustes → MCP** (pide tu contraseña) y da al cliente solo la URL del servidor, `https://<your-domain>/api/mcp`, sin token.

1. El cliente se registra solo y abre una página de consentimiento en tu instancia (inicia sesión antes si hace falta).
2. La página muestra el nombre del cliente y **adónde se envía tu respuesta**. Elige los módulos y niveles y luego **Permitir**: se pide tu contraseña en cada aprobación.
3. La conexión aparece en la tabla de tokens con una insignia **OAuth**. Edita sus permisos o revócala ahí como cualquier token; revocar desconecta al cliente.

Los tokens de acceso duran una hora y se renuevan en segundo plano; una conexión sin uso durante 30 días tiene que iniciar sesión de nuevo. Si alguien más reutiliza alguna vez un token de renovación, la conexión se revoca y se te notifica.

::: warning Los clientes remotos necesitan HTTPS público
claude.ai y ChatGPT se conectan desde sus propios servidores, así que la instancia debe ser accesible por HTTPS público ([modo Web](/es/config/network)). Aprueba solo una página de consentimiento que hayas abierto tú mismo, justo ahora: un enlace enviado por otra persona puede llevar el nombre de cualquier cliente.
:::

::: tip ¿Actualizaste desde 0.0.15 o anterior con `otw update`?
OAuth necesita una nueva ruta en `deploy/Caddyfile`. Consulta [Actualización](/es/guide/updating#oauth-caddyfile).
:::

## Cómo ven la app los agents

Los agents obtienen cuatro herramientas de pasarela:

- **`otw_catalog`**: lista los módulos y operaciones que el token puede llamar. Solo aparecen los módulos concedidos. El listado de un módulo muestra el método, la ruta, los parámetros de query y los campos de primer nivel del cuerpo de cada operación; `endpoint` (`POST /api/backtest/run`) devuelve el esquema completo del cuerpo de esa operación.
- **`otw_read`**: operaciones de lectura (necesitan al menos *lectura* en el módulo).
- **`otw_compute`**: operaciones marcadas *(compute)* en el catálogo, que responden una pregunta y no almacenan nada: un backtest, un barrido de parámetros, métricas de riesgo. Necesitan *lectura + escritura* como cualquier POST, pero tu cliente no te pedirá aprobar un cálculo.
- **`otw_write`**: operaciones de creación y actualización (necesitan *lectura + escritura*); las operaciones de **borrado** requieren *completo* en el módulo.

Las respuestas que traen texto del exterior (artículos de feeds, correo entrante) llegan al agent dentro de un bloque etiquetado que le indica tratar el contenido como datos e ignorar cualquier instrucción oculta en él.

La tabla de tokens en Ajustes muestra la hora de último uso de cada token, para que puedas detectar y revocar los obsoletos. También se puede dar a un token una fecha de caducidad al crearlo o editarlo: pasada esa fecha deja de funcionar en todas partes, incluido el agent de la app.
