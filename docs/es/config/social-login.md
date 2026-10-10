# Social login

Inicia sesión con una cuenta de Google, Microsoft, GitHub u OpenID Connect (Authentik, Keycloak,
Authelia, Zitadel…).

El sign-in queda bloqueado a la cuenta que vincules, por el id de usuario del proveedor, no por correo. Cualquier otra
cuenta del mismo proveedor se rechaza. Si la autenticación en dos factores está activada, el código
se sigue pidiendo.

Todos los ajustes están en **Ajustes → Seguridad → Social login**. Cada cambio pide tu
contraseña.

## Requisitos {#requirements}

- **URI de redirección**: `<address of OTW>/auth/social`, p. ej. `https://otw.example.com/auth/social`.
  Ajustes muestra el valor exacto para la dirección en la que estás (haz clic para copiar). Debe coincidir con
  la dirección del navegador exactamente: esquema, host y puerto.
- **Google**: `https://` y un nombre de dominio. Se rechazan `http://` plano y las direcciones IP directas,
  salvo `localhost` / `127.0.0.1`.
- **Microsoft**: `https://`, o `http://localhost`.
- **GitHub** y proveedores autoalojados: lo que ellos permitan.

Una instancia a la que se accede por HTTP plano en una dirección de LAN debe cambiar antes a `lan_https` o `web`
para Google y Microsoft ([Red](/es/config/network)).

## Configuración {#setup}

### 1. Registrar OTW en el proveedor

::: details Google
1. [Consola de Google Cloud](https://console.cloud.google.com/) → **Google Auth Platform**.
   En el primer uso, rellena el nombre de la app y el correo de soporte, audiencia **Externa**.
2. **Audiencia**: mientras la app esté en *Pruebas*, solo pueden iniciar sesión los usuarios de prueba listados. Añade tu
   cuenta de Google ahí.
3. **Clientes → Crear cliente**, tipo **Aplicación web**.
4. **URI de redirección autorizados**: la URI de redirección de Ajustes. Deja vacío *Orígenes de JavaScript
   autorizados*.
5. Copia el ID de cliente y el secreto de cliente. Google puede tardar unos minutos en aplicar una nueva
   URI de redirección.
:::

::: details Microsoft
1. [Centro de administración de Microsoft Entra](https://entra.microsoft.com/) → **Registros de aplicaciones → Nuevo
   registro**.
2. **Tipos de cuenta compatibles**: incluye las cuentas personales si inicias sesión con una
   (Outlook.com, Hotmail, Xbox).
3. **Autenticación → Agregar una plataforma → Web**: la URI de redirección de Ajustes.
4. **Certificados y secretos → Secretos de cliente → Nuevo secreto de cliente**. Copia el **Valor**, se
   muestra una sola vez. Caduca (24 meses como máximo), consulta [Secreto caducado](#secret-expired).
5. Copia el **ID de aplicación (cliente)** de **Información general**.
6. Tenant en OTW: `common` (por defecto, cualquier cuenta), `consumers` (solo personales),
   `organizations` (solo trabajo o centro educativo), o tu ID de tenant o dominio.
:::

::: details GitHub
1. GitHub → **Settings → Developer settings → OAuth Apps → New OAuth App**.
2. **Authorization callback URL**: la URI de redirección de Ajustes.
3. Copia el ID de cliente, luego **Generate a new client secret** y cópialo.
:::

::: details OpenID Connect (autoalojado)
1. Crea un cliente OpenID Connect confidencial: flujo de código de autorización, scopes
   `openid email profile`, la URI de redirección de Ajustes.
2. Copia el ID de cliente, el secreto de cliente y la **URL del emisor**: la dirección que sirve
   `/.well-known/openid-configuration`, p. ej. `https://auth.example.com/application/o/otw` en
   Authentik.
3. El emisor debe ser `https://` (`http://` solo en localhost) y debe coincidir exactamente con el campo `issuer`
   de ese documento.
:::

### 2. Introducirlo en OTW

Elige el proveedor, pega el ID de cliente y el secreto (y el tenant o la URL del emisor) y **Guarda**.
OTW contacta con el proveedor al guardar: un tenant o emisor incorrecto falla aquí, no al iniciar sesión.

### 3. Vincular tu cuenta

**Vincular una cuenta** te lleva al proveedor para elegir la cuenta. De vuelta en Ajustes, la
tarjeta muestra *Bloqueado a …*. La página de sign-in tiene ahora un botón **Continuar con …**.

### 4. Generar códigos de recuperación {#recovery-codes}

**Códigos de recuperación → Generar**: diez códigos de un solo uso, mostrados una vez. Te permiten entrar cuando la
cuenta del proveedor está bloqueada, eliminada o inaccesible. Guárdalos fuera de este servidor. Un nuevo conjunto
cancela el anterior.

### 5. Desactivar el sign-in con contraseña (opcional) {#password-off}

Posible una vez vinculada una cuenta y con códigos de recuperación. **Sign-in con contraseña → Desactivar**:
el formulario de acceso rechaza contraseñas, solo funcionan la cuenta vinculada y los códigos de recuperación.

Se vuelve a activar solo al desvincular, al quitar, al iniciar sesión con un código de recuperación y al restablecer
la contraseña desde el host.

## Reversión {#rollback}

### Con la sesión aún iniciada

- **Sign-in con contraseña → Activar**: las contraseñas vuelven a funcionar, el social login se mantiene.
- **Desvincular**: el social login se detiene, las contraseñas vuelven a funcionar. Los ajustes del proveedor se conservan.
- **Quitar**: se eliminan los ajustes del proveedor y el vínculo, las contraseñas vuelven a funcionar.

### Cuenta del proveedor inutilizable

Página de sign-in → **Usar un código de recuperación**. Entras, el sign-in con contraseña se reactiva y
se abre Ajustes → Seguridad. Desvincular o vincular otra cuenta ahí no pide la
contraseña durante los cinco minutos siguientes.

### También perdiste los códigos de recuperación

En el host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Elimina el vínculo, activa el sign-in con contraseña y cierra todas las sesiones. Si también perdiste
la contraseña, ejecuta `reset-password` a continuación ([Olvidé mi
contraseña](/es/guide/troubleshooting#forgot-password)). `list-users` imprime el nombre de usuario.

### Volver a la versión anterior {#downgrade}

Esta versión añade la migración `0145_social_login`. Una versión anterior se niega a arrancar sobre una
base de datos con una migración que no conoce, así que elimínala primero:

1. Haz una copia de la base de datos ([Copia y restauración](/es/guide/backup-restore)).
2. Desde `deploy/`:

   ```bash
   docker compose --env-file .env --env-file network.env exec -T postgres \
     psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -c "
       BEGIN;
       DROP TABLE IF EXISTS recovery_codes;
       DROP TABLE IF EXISTS social_login;
       ALTER TABLE users DROP COLUMN IF EXISTS password_login;
       DELETE FROM _sqlx_migrations WHERE version = 145;
       COMMIT;"
   ```

3. Instala la versión anterior ([Actualización](/es/guide/updating)): compilación desde fuente,
   `git reset --hard <previous release commit>` y luego `up -d --build`; instalación con imágenes, el
   `deploy/` y las imágenes de la versión anterior.

Las contraseñas funcionan en la versión anterior sea cual sea el valor del interruptor. Los ajustes del proveedor y
los códigos de recuperación se eliminan.

::: warning
Esto elimina solo la migración 145. Si hay instalada una versión posterior con más migraciones,
restaura en su lugar la copia hecha antes de esa actualización.
:::

El paso 2 sin el paso 3 restablece el social login: la versión actual recrea las tablas vacías
en su siguiente arranque.

## Errores {#errors}

| Mensaje | Solución |
|---|---|
| `redirect_uri_mismatch` (Google), `AADSTS50011` (Microsoft) | La URI de redirección registrada difiere de la dirección del navegador. Cópiala de nuevo desde Ajustes. |
| *The OAuth client was not found* / `invalid_client` | ID de cliente o secreto incorrectos, o el secreto caducó. |
| *the provider calls itself …* | La URL del emisor difiere del `issuer` en el `/.well-known/openid-configuration` del proveedor. |
| *this … account is not the one linked to this instance* | Se eligió otra cuenta en el proveedor. |
| *this sign-in was started in another browser or has expired* | Pasaron más de diez minutos, o el sign-in terminó en otro navegador. Empieza de nuevo. |
| *the ID token has expired* | El reloj del servidor es incorrecto. Corrige la hora del host (NTP). |

### Secreto caducado {#secret-expired}

El sign-in falla con `invalid_client` o con un mensaje sobre un secreto caducado. Crea un nuevo secreto
en el proveedor, luego **Edita** en Ajustes, pégalo y **Guarda**. El vínculo se conserva. ¿Bloqueado?
Inicia sesión primero con un código de recuperación.
