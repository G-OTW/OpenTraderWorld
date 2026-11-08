# Seguridad de la cuenta

OTW tiene una sola cuenta y ningún correo de restablecimiento de contraseña. Lo que la protege es la contraseña (o una
cuenta social vinculada), un segundo factor opcional y el hecho de que nadie más que tú puede
acceder a la máquina. Esta página
cubre lo que puedes activar y qué hacer cuando algo sale mal.

Todo lo de aquí vive en **Ajustes → Seguridad**, excepto la contraseña en sí, que se queda en
**Ajustes → Cuenta**.

## Autenticación en dos factores {#totp}

Un código de seis dígitos de una app en tu móvil, que se pide después de la contraseña. Es lo que una
contraseña robada por sí sola no puede superar, y es lo más útil que puedes activar
antes de abrir la app a internet.

::: tip No se te envía nada
Esto es **TOTP** (RFC 6238), no un código enviado por SMS o correo bajo demanda. Tu autenticador y
el servidor comparten un secreto una sola vez, en la configuración, y luego cada uno calcula el mismo código a partir de la hora
actual. Sin SMS, sin correo, sin servicios de terceros, y funciona con la máquina sin conexión.
:::

### Activarlo

1. **Ajustes → Seguridad → Configurar**. La app muestra un código QR, el enlace `otpauth://` que
   hay detrás y el propio secreto.
2. Añádelo a cualquier app de autenticación: Google Authenticator, Aegis, Ente Auth, 1Password,
   Bitwarden, Proton Pass, la que ya uses. Escanea el código QR, pega el enlace
   o escribe el secreto a mano.
3. Escribe los seis dígitos que muestra y confirma.

Nada sobre el sign-in cambia hasta que ese último paso tiene éxito. Un código que no puedes generar
nunca se convierte en requisito, así que una configuración a medias no puede dejarte fuera.

### Iniciar sesión después

Introduce tu usuario y contraseña como antes; la app pedirá entonces el código. Cada código sirve
una vez y dura 30 segundos, con algo de tolerancia a cada lado para un móvil cuyo reloj se haya
desajustado.

### Desactivarlo

**Ajustes → Seguridad → Desactivar**, que vuelve a pedir tu contraseña. Hazlo antes de
borrar un móvil, y configúralo de nuevo en el nuevo.

### Si pierdes el autenticador {#totp-lost}

No hay código de respaldo ni correo de recuperación, por la misma razón por la que no hay formulario de
restablecimiento de contraseña. Recupera desde la shell de la máquina:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

Se eliminan el segundo factor y su secreto y se cierran todas las sesiones. Inicia sesión con
tu contraseña y configúralo de nuevo en el nuevo dispositivo.

::: warning Conserva una vía de vuelta
Guarda el secreto en tu gestor de contraseñas al configurarlo, o mantén activada la copia
de seguridad propia de tu autenticador. De lo contrario, la única vía de vuelta es el acceso por shell a la máquina.
:::

## Bloqueo de trading {#trading-lock}

Con el bloqueo activo, haber iniciado sesión no basta para enviar órdenes. La primera orden
tras iniciar sesión pide un código de tu autenticador, una sola vez, y el trading queda
abierto en todas las cuentas de bróker en ese navegador hasta cerrar sesión. Otro navegador o
un nuevo inicio de sesión lo vuelve a pedir.

- **Activarlo**: **Ajustes → Seguridad → Bloqueo de trading → Activar**. La autenticación en
  dos factores debe estar configurada antes, el bloqueo se abre con ella.
- **Qué cubre**: enviar una orden, modificar una orden pendiente y cerrar una posición.
  Cancelar una orden, o todas con **Cancelar todas las órdenes**, nunca lo pide: el bloqueo
  nunca te impide reducir el riesgo.
- **Desactivarlo**: pide tu contraseña y un código, como las demás
  [acciones confirmadas](#step-up), para que una sesión robada no pueda quitarlo primero.
- **Códigos erróneos**: cinco desde un navegador bloquean ese navegador hasta 15 minutos.
  Desde el tercero se envía una notificación como una alerta de inicio de sesión.
- Los agentes de IA y los workflows del Automator no pueden enviar órdenes en absoluto, con
  bloqueo o sin él.

### Reiniciarlo {#trading-lock-reset}

¿Perdiste el autenticador? Quita el segundo factor con `disable-totp`
[como arriba](#totp-lost). Con el bloqueo activo, esto también desactiva el bloqueo y el
trading en **todas** las cuentas de bróker, para que nada quede abierto sin factor.
Desactivar la autenticación en dos factores desde Ajustes hace lo mismo. Para volver a operar:
inicia sesión, configura la autenticación en dos factores, reactiva el trading cuenta por
cuenta en **Ajustes → Brokers** (cada una pide tu contraseña) y vuelve a activar el bloqueo.

## Social login {#social}

Sign-in con Google, Microsoft, GitHub u OpenID Connect, bloqueado a una cuenta vinculada, con
códigos de recuperación y un interruptor opcional para desactivar las contraseñas. Consulta
[Social login](/es/config/social-login).

## Sesiones activas {#sessions}

**Ajustes → Seguridad** lista todos los navegadores que tienen la sesión iniciada en la cuenta, con la
dirección desde la que llegaron y cuándo se usaron por última vez. Aquel en el que estás leyendo esto está marcado.

- **Cerrar** finaliza una de ellas.
- **Cerrar todas las demás sesiones** finaliza todas menos la tuya, que es lo que debes pulsar si iniciaste sesión
  en algún sitio donde no debías, o no estás seguro.

Un sign-in desde una dirección que la cuenta nunca había usado también genera una notificación. Llega
a la campana y se envía a cualquier canal concedido al productor **Seguridad (inicios de sesión,
tokens, exposición)** en **Ajustes → Notificaciones** (una concesión comodín ya lo cubre). Se dispara una vez
por dirección, no en cada sign-in.

Con **Remember me** marcado al iniciar sesión, una sesión dura una semana. Sin él, dura un día
y termina al cerrar el navegador. En los modos expuestos a la red (**LAN + HTTPS** y **Público**)
una sesión además termina tras un día sin actividad, para que un navegador abierto en una máquina
que dejaste no siga con la sesión iniciada indefinidamente. En localhost y LAN plana solo se aplica
el límite fijo.

## La petición de contraseña que vuelve {#step-up}

Un puñado de acciones piden tu contraseña de nuevo aunque ya hayas iniciado sesión:

- generar un token de acceso para un AI agent (**Ajustes → MCP**)
- activar el sign-in OAuth para AI agents, y aprobar cada página de consentimiento OAuth
- cambiar el modo de red (**Ajustes → Red**)
- escribir un valor en la **Bóveda**
- descargar una copia parcial **con credenciales incluidas**
- desactivar el segundo factor, o configurarlo de nuevo mientras está activo
- desactivar el bloqueo de trading
- cambiar el [social login](/es/config/social-login): los ajustes del proveedor, vincular o
  desvincular la cuenta, nuevos códigos de recuperación, activar o desactivar el sign-in con contraseña

Estas acciones crean una credencial, cambian lo que el mundo exterior puede alcanzar, o te entregan todos los
secretos almacenados en un solo archivo. Una confirmación las cubre todas durante cinco minutos, así que una serie de
cambios pide la contraseña una vez, y la confirmación pertenece al navegador que la dio. Si el segundo
factor está activado, la petición también pide un código.

Activar el trading en una cuenta de bróker, cambiar sus límites de riesgo o la **Regla de bloqueo**, y
levantar un bloqueo piden la contraseña cada vez: la ventana de cinco minutos no cuenta para
ellos.

## Reglas de contraseña {#password}

Se define o cambia en **Ajustes → Cuenta**, que siempre pide primero la contraseña actual
y, si tiene éxito, cierra todas las sesiones, incluida la actual.

Una contraseña debe tener **al menos 12 caracteres** y no ser una que ya aparezca en
listas públicas de filtraciones. Esa comprobación ignora la decoración que la gente añade para saltarse una regla, así que
`P@ssw0rd!2024` se rechaza por la misma razón que `password`. Las secuencias (`abcdefghijkl`) y
cualquier cosa que contenga el nombre de tu cuenta también se rechazan.

La contraseña más fácil que pasa es unas cuantas palabras sin relación: `fennel-ladder-oxide-73` se
acepta, es corta y fácil de teclear. No hay regla sobre mezclar símbolos y dígitos, porque
lo que importa es la longitud.

¿La olvidaste? Consulta [Olvidé mi contraseña](/es/guide/troubleshooting#forgot-password).

## Tiempo límite de petición {#timeout}

**Ajustes → Seguridad** también define cuánto tiempo puede ejecutarse una sola petición antes de que el servidor la
detenga. El valor por defecto es 120 segundos.

Existe para que una petición atascada no mantenga una conexión abierta para siempre. **No** es un límite
de frecuencia: nada cuenta con qué frecuencia llamas a la app, y un AI agent que use la API intensamente
no se ve afectado. Los flujos de precios en vivo y la vista de registros tampoco se ven afectados, porque el límite cubre la
producción de una respuesta, no la vida de un flujo.

Súbelo si un backtest largo o una importación grande se corta con un mensaje de tiempo agotado.

## Primer arranque en una instancia expuesta a la red {#setup-token}

Cuando la primera cuenta se crea en una instancia ya accesible desde la red
(**LAN + HTTPS** o **Público**), el asistente de configuración pide un **token de configuración**. Consulta
[Red y acceso remoto](/es/config/network#setup-token).
