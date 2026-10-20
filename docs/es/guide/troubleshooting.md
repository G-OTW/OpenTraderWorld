# Solución de problemas

| Síntoma | Causa probable / solución |
|---|---|
| `port is already allocated` | El puerto (80/443/5454) lo usa otra cosa. Vuelve a ejecutar `./setup.sh` y elige otro puerto. |
| Chrome/Edge no abre la app pero Safari sí | El navegador fuerza la dirección a `https://`, que los modos HTTP plano no sirven. Escribe `http://` explícitamente, o cambia al [modo LAN + HTTPS](/es/config/network#lan-https). |
| Funciona en `localhost` pero no mediante la IP de la máquina (macOS) | El firewall de macOS bloquea las conexiones entrantes de Docker. Ajustes del Sistema → Red → Firewall → Opciones… → configura **Docker** en *Permitir conexiones entrantes*. |
| LAN + HTTPS: certificado no emitido | Revisa `docker compose logs caddy`. El token de DuckDNS/Cloudflare debe ser válido y el dominio estar escrito exactamente. |
| LAN + HTTPS: el dominio no resuelve en algunos dispositivos | Tu resolvedor bloquea respuestas con IP privada (protección contra DNS rebind). Consulta [las soluciones](/es/config/network#dns-rebind). |
| El asistente de configuración nunca aparece / `core: offline` en la barra superior | El core no alcanza Postgres. Revisa `docker compose logs core` y `logs postgres`; verifica que `DATABASE_URL` coincida con `POSTGRES_PASSWORD` en `deploy/.env`. |
| Error de `POSTGRES_PASSWORD` al arrancar | Falta `deploy/.env` o está vacío. Ejecuta `./setup.sh`, o copia `.env.example` a `.env` y rellénalo. |
| Modo público: certificado HTTPS no emitido | El DNS del dominio debe resolver a este servidor, y los puertos 80/443 deben ser accesibles desde internet. |
| Los cambios de código no se reflejan | Recompila: `docker compose up --build -d`. |
| Bloqueado, contraseña olvidada | Restablécela desde la shell del host, consulta [Olvidé mi contraseña](#forgot-password). |
| No se puede acceder a la app tras elegir el modo de red equivocado | Edita `deploy/network.env` a mano y reinicia, consulta [cambiar el modo desde la CLI](/es/config/network#change-mode-cli). |

## Olvidé mi contraseña {#forgot-password}

No hay correo de restablecimiento ni formulario de restablecimiento sin autenticar: OTW se ejecuta en tu propio servidor, así que cualquier endpoint que pudiera cambiar una contraseña sin haber iniciado sesión sería una puerta de entrada. El restablecimiento vive en la **shell del host**, y el enlace *¿Olvidaste tu contraseña?* de la página de sign-in detalla los mismos pasos.

Abre una shell en la máquina que ejecuta OTW e imprime una contraseña de un solo uso:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core reset-password USERNAME
```

Inicia sesión con ella; la app te pide una nueva de inmediato.

| Caso | Qué ejecutar |
|---|---|
| También olvidaste el usuario | `docker exec opentraderworld-core-1 /app/otw-core list-users` |
| Elegir tú la contraseña | `printf '%s' 'my-new-password' \| docker exec -i opentraderworld-core-1 /app/otw-core reset-password USERNAME --stdin` |
| El contenedor tiene otro nombre | `docker ps`, y usa el del core en lugar de `opentraderworld-core-1`. |
| No usas Docker | Ejecuta el binario `otw-core` con los mismos argumentos y `DATABASE_URL` definido. |

Nunca pases una contraseña como argumento de línea de comandos: la línea de comandos de un proceso es legible en el host, por eso existe `--stdin`.

Notas: un restablecimiento **cierra la sesión en todos los dispositivos**, y no se pierde nada. La bóveda y las claves de proveedor almacenadas están selladas con `OTW_SECRET_KEY`, no con tu contraseña.

## Perdiste tu autenticador {#lost-authenticator}

El mismo principio que con la contraseña: recupera desde la shell del host.

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

Se elimina el segundo factor y se cierran todas las sesiones. Inicia sesión con tu contraseña
y luego configúralo de nuevo en el nuevo dispositivo desde **Ajustes → Seguridad**. Consulta
[Autenticación en dos factores](/es/config/security#totp).

## Bloqueado en el social login {#social-locked}

La cuenta del proveedor vinculado está bloqueada, eliminada o inaccesible: en la página de sign-in, **Usar un
código de recuperación**. El sign-in con contraseña vuelve a activarse.

¿También perdiste los códigos de recuperación? En el host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Elimina el vínculo, activa el sign-in con contraseña y cierra todas las sesiones. ¿También perdiste la contraseña?
[Olvidé mi contraseña](#forgot-password). Detalles: [Social login](/es/config/social-login#rollback).

## Una petición larga se corta {#request-timeout}

Un backtest, barrido o importación grande que responde con *this request took longer than the
Ns limit* alcanzó el tiempo límite de petición, no es un fallo. Súbelo en **Ajustes → Seguridad**; se aplica
de inmediato, sin reiniciar. Consulta [Tiempo límite de petición](/es/config/security#timeout).

## Leer los registros

```bash
cd deploy
docker compose ps              # are all containers up?
docker compose logs -f core    # API server
docker compose logs -f caddy   # proxy / certificates
docker compose logs -f postgres
```

La app también mantiene su propia vista de registros en **Ajustes → Registros** (con búsqueda y nivel de captura configurable).

## Empezar de cero

::: danger Esto borra todos los datos
```bash
cd deploy
docker compose down -v
./setup.sh
```
:::

## ¿Sigues atascado?

Abre un issue en [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues) con el síntoma y las líneas de registro relevantes.
