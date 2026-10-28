# Control externo (chat)

Maneja OpenTraderWorld desde **Telegram, Slack o Discord**. Un mensaje que envías a tu bot se convierte en una ejecución de una de tus [personas de agent](/es/modules/agent), respondida en el chat, con exactamente el acceso que permite el token que elegiste.

**Está desactivado por defecto**, y no añade ningún sistema de permisos propio: el techo es un [token MCP](/es/config/ai-agents), el mismo que usa el asistente de la app.

## Nada nuevo escucha en tu máquina

Los tres transportes **salen hacia fuera**: long polling de Telegram, Socket Mode de Slack, el gateway de Discord. No hay URL pública que publicar, ni puerto que abrir, ni ruta entrante que atacar, así que funciona sin cambios en la instalación por defecto solo localhost y detrás de NAT.

El canal que ya usas para notificaciones solo puede **enviar** (la URL de un webhook de Slack o Discord es de solo escritura). Recibir necesita un bot de verdad, así que un vínculo guarda su propia credencial:

| Plataforma | Credencial a pegar | En la plataforma |
|---|---|---|
| **Telegram** | el token del bot de BotFather | nada más |
| **Slack** | **ambos** tokens, `xapp-…` y `xoxb-…`, separados por un espacio o un salto de línea | Socket Mode activado, evento `message.im`, scope `chat:write` |
| **Discord** | el token del bot | el intent de **Direct Messages** (el intent privilegiado de contenido de mensajes no hace falta para los DM) |

## Configurar uno

1. **Ajustes → Notificaciones**: crea el canal si no tienes ninguno. Es la vía de respuesta.
2. **Ajustes → MCP**: crea o edita un token, define sus niveles por módulo y marca **Permitir acceso externo**.
3. **Ajustes → Control externo**: **Nuevo vínculo**, elige el canal, la persona y ese token, pega la credencial del bot (o enchúfala desde la [Bóveda](/es/config/settings#vault)), opcionalmente define el proveedor y el modelo con los que empiezan los chats nuevos y activa el vínculo.
4. Activa el interruptor de la sección. El vínculo muestra **Conectado** en pocos segundos.
5. **Emparejar**: haz clic en *Emparejar* y luego envía el código de 6 dígitos a tu bot **desde la cuenta que debe poder manejarlo**. Funciona una vez, y dura lo que indique *Duración del código de emparejamiento* (una hora por defecto, de 5 minutos a un día).

Hasta que alguien esté emparejado, el bot no responde a nadie, ni siquiera a ti.

## Qué modelo responde

Cada chat lleva su propio proveedor y modelo, exactamente como una conversación en la app. El vínculo define el **valor por defecto con el que empieza un chat nuevo**: un tramo por móvil suele merecer un modelo más barato y rápido que el que la misma persona usa en el navegador. Déjalo vacío y el chat hereda el de la persona.

Cambia el valor por defecto en el formulario del vínculo. Cambia un chat desde el propio chat:

- `/provider` lista los proveedores configurados, `/provider 2` o `/provider openrouter` cambia este chat a uno (lo que restablece el modelo al predeterminado de ese proveedor, ya que un id de modelo pertenece a un solo fabricante).
- `/model` lista los modelos de ese proveedor, `/model 3` elige por posición y `/model haiku` elige por texto cuando exactamente un id coincide.

La elección se queda en ese chat y no mueve nada más. `/new` descarta la conversación, así que la siguiente vuelve a empezar con el valor por defecto del vínculo.

## Derechos

El token decide todo lo que una respuesta puede tocar: *sin acceso*, *lectura*, *lectura + escritura*, *completo* por módulo, exactamente como en la [página de MCP](/es/config/ai-agents#security-model). La marca `external` no amplía nada, solo dice que se puede acceder a ese margen desde fuera.

- **Un token por vínculo, un vínculo por canal.** Revocar un token detiene ese vínculo y nada más, y el registro sigue indicando por qué vía entró una llamada.
- **Usa un token dedicado**, y empieza con solo lectura. Puedes ampliarlo después sin volver a emparejar.
- Un token **caducado** o que pierde la marca detiene el vínculo en el siguiente mensaje, no en el siguiente reinicio.

## Quién puede manejarlo

Un chat es un lugar, no una identidad, así que la autoridad se vincula al **id del remitente** de la plataforma:

- Solo se responde a un remitente emparejado. Cualquier otro es **ignorado sin respuesta**, algo deliberado: un rechazo le dice a un extraño que el bot existe.
- Los remitentes emparejados se listan en el vínculo. Haz clic en uno para quitarlo.
- **Solo mensajes directos.** Un grupo permitiría que varias personas escribieran en el prompt de un agent que puede tener una concesión de escritura.

## Las escrituras siempre preguntan

Cada escritura se te plantea en el chat con el método, la ruta y el cuerpo exactos, y espera una palabra. Solo `yes`, `y`, `ok`, `okay`, `approve`, `oui` o `go` la aprueban; cualquier otra cosa, o el silencio, la rechaza y se le dice al agent que fue rechazada.

El ajuste **aprobar escrituras automáticamente** de la persona no se traslada. Se marcó en una sesión autenticada en la app; no te sigue al móvil.

## Por qué pasa un mensaje

En orden, por todos ellos:

1. el interruptor global en **Ajustes → Control externo**
2. que el vínculo esté activado
3. que el token siga llevando `external` y no esté caducado
4. un chat directo
5. que el remitente esté en la lista de permitidos
6. un límite de tasa de 12 mensajes por minuto por vínculo

Después la propia ejecución está sujeta a la lista de permitidos del catálogo MCP: las rutas de cuenta, red, secretos, almacenamiento de archivos y borrado de datos son inaccesibles diga lo que diga el token, y también lo es la gestión de vínculos. Ningún agent puede crear un vínculo, generar un código de emparejamiento ni ampliar su propio alcance.

## Notas de seguridad

- **El token del bot es el acceso.** Quien lo tenga puede hablar con tu instancia al nivel de ese vínculo, con la lista de remitentes permitidos aún por medio. Guárdalo en la [Bóveda](/es/config/settings#vault) y empieza con solo lectura.
- **La inyección de prompts es el riesgo real**, no el transporte. El contenido que lee el agent (correo, feeds, webhooks entrantes) puede llevar instrucciones. Lo que lo contiene es lo mismo que lo contiene en la app: la lista de permitidos del catálogo, los niveles del token y la confirmación de escritura anterior.
- Los **códigos de emparejamiento** son de seis dígitos, de un solo uso y con tiempo limitado, y solo existen entre hacer clic en *Emparejar* y canjearse. Acorta la ventana en la cabecera de la sección si un código va a quedarse en pantalla.
- Las credenciales de los bots se sellan en reposo y nunca se imprimen, ni siquiera en los mensajes de error.
- Desactivado por completo en el [modo demo](/es/guide/demo).

## Límites

- **Solo texto.** Sin gráficos ni archivos; un gráfico sigue viviendo en la app.
- **Sin streaming token a token.** Las plataformas de chat solo ofrecen ediciones de mensajes, y las limitan, así que la respuesta llega en bloques de aproximadamente un segundo y medio y se divide en el tope de la plataforma (4096, 3000 y 2000 caracteres).
- Comandos: `/new` inicia una conversación nueva para ese chat, `/provider` y `/model` listan y cambian con qué responde ese chat, `/whoami` muestra el id con el que estás emparejado, `/help`.
- Un reinicio descarta una confirmación de escritura pendiente. Nada se ejecuta sin confirmar, simplemente se te vuelve a preguntar.
