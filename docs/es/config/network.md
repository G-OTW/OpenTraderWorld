# Red y acceso remoto

OpenTraderWorld controla **quién puede acceder a la app** mediante cuatro modos de red. Tras la instalación es **solo localhost**: nada de tu red puede conectarse hasta que lo cambies.

La forma admitida de cambiar de modo es desde la app: **Ajustes → Red**. Al guardar se muestra el comando de reinicio exacto que debes ejecutar en el host (la app no puede reiniciar sus propios contenedores); la pila queda brevemente fuera de línea mientras se recrean los contenedores.

## Los cuatro modos

| Modo | Accesible por | Protocolo | Úsalo cuando |
|---|---|---|---|
| **Solo localhost** | esta máquina | HTTP | Por defecto. El más seguro, nada de la red puede conectarse. |
| **Red local (LAN)** | dispositivos de tu red, mediante la IP de la máquina | HTTP plano | Acceso rápido por LAN; válido para redes domésticas de confianza. Los navegadores pueden avisar o forzar HTTPS, y rechazan el micrófono, por lo que el [control por voz](/es/config/voice#https) no funciona desde otros dispositivos. |
| **LAN + HTTPS** | dispositivos de tu red, mediante un dominio real | HTTPS, certificado de confianza | Acceso por LAN sin avisos del navegador. Nada expuesto a internet. |
| **Público (Web)** | cualquiera, en tu dominio | HTTPS (Let's Encrypt) | Quieres acceso desde cualquier lugar y aceptas la exposición pública. |

## LAN + HTTPS (sin avisos del navegador) {#lan-https}

Los navegadores rechazan o avisan cada vez más en los sitios HTTP plano. Este modo sirve la app a todos los dispositivos de tu red con un **certificado real y de confianza pública**: sin avisos, sin instalar nada en los dispositivos cliente y **sin exponer nada a internet**. La propiedad del dominio se demuestra con un registro DNS (desafío ACME DNS-01), no con una conexión entrante, y el dominio resuelve a la IP privada de LAN de tu máquina.

Configúralo en la instalación (`./setup.sh`, modo `3`) o más tarde en **Ajustes → Red → Red local (LAN) + HTTPS**:

1. Inicia sesión en [duckdns.org](https://www.duckdns.org) (gratis), añade un subdominio (p. ej. `myotw.duckdns.org`) y copia el token de tu cuenta, o usa tu propio dominio en Cloudflare con un token de API con permiso de edición de DNS.
2. Introduce el dominio + token, más la IP de LAN de tu máquina (con DuckDNS el registro se apunta a ella automáticamente; en Cloudflare crea tú el registro A).
3. Aplica con el comando de reinicio mostrado. La primera petición puede tardar ~30 s mientras se emite el certificado.

Después abre `https://myotw.duckdns.org` desde cualquier dispositivo de tu red.

::: warning Notas
- Los certificados emitidos aparecen en los registros públicos de Certificate Transparency, por lo que el **nombre** del dominio es visible públicamente (la app en sí sigue siendo solo LAN).
- El token de DNS se guarda en `deploy/dns.env`: nunca subas ni compartas ese archivo.
- Este modo usa los puertos **80 + 443** en lugar del puerto personalizado.
:::

### Si el dominio no resuelve en algunos dispositivos {#dns-rebind}

Algunos routers/resolvedores de ISP descartan en silencio las respuestas DNS que apuntan a una IP privada ("protección contra DNS rebind"). Soluciones, de mejor a peor:

1. **Permite el dominio** en los ajustes de tu router/DNS.
2. **Activa el DNS seguro (DNS sobre HTTPS)** en el navegador. Chrome: Configuración → Privacidad y seguridad → Seguridad → *Usar DNS seguro* → Cloudflare; Firefox: Ajustes → Privacidad → *DNS sobre HTTPS* → Protección máxima.
3. **Solución con el archivo hosts** (por máquina, y los móviles no pueden hacerlo). Asigna el dominio a la IP de LAN del servidor:

   ```bash
   # macOS / Linux, then flush the cache (macOS only):
   echo "192.168.1.50 myotw.duckdns.org" | sudo tee -a /etc/hosts
   sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
   ```

   En Windows, edita `C:\Windows\System32\drivers\etc\hosts` como administrador, añade la misma línea y ejecuta `ipconfig /flushdns`. El archivo hosts siempre prevalece sobre el DNS, así que elimina la línea si cambia la IP del servidor.

## Público (Web) {#public}

Expone la app en tu propio dominio con HTTPS automático.

**Requisitos previos:**

1. Un **dominio** con un **registro A / AAAA** público que apunte a la IP pública de tu servidor.
2. TCP **80** y **443** entrantes alcanzando el servidor: ábrelos en el firewall del router o de la nube y redirige puertos si estás tras NAT. El puerto 80 es necesario para el certificado (desafío HTTP-01) y redirige a HTTPS.

Elige el modo `4` en `./setup.sh` o cambia en **Ajustes → Red**. Caddy obtiene y renueva automáticamente un certificado de Let's Encrypt en la primera petición (~30 s).

::: danger Cualquiera puede acceder a la página de sign-in una vez activado
Actívalo solo cuando ya exista tu cuenta de administrador y con una contraseña robusta. Activa antes la
[autenticación en dos factores](/es/config/security#totp). Mantén `deploy/.env` en secreto.
Vuelve a un modo privado cuando quieras en Ajustes → Red.
:::

## El token de configuración {#setup-token}

En **LAN + HTTPS** y **Público**, el asistente de primer arranque pide un **token de configuración** antes de
crear la primera cuenta.

El motivo es una carrera: esos modos responden a la red, y el asistente está abierto hasta que existe una cuenta.
Sin token, quien cargue la página primero se convierte en administrador, lo cual es un
riesgo real en el intervalo mientras se propaga el DNS, y de nuevo si alguna vez se recrea un volumen de base de datos.

El core genera el token al arrancar y lo imprime en su registro:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env logs core | grep "setup token"
```

Pégalo en el campo adicional que muestra el asistente. Se genera uno nuevo en cada reinicio, así que un
token abandonado deja de funcionar en cuanto el contenedor se reinicia.

No lo verás si instalaste con `./setup.sh`: crea la cuenta a partir de
`deploy/.env` en el primer arranque, antes de que nada pueda alcanzar el asistente. El token solo aparece
cuando todavía no existe ninguna cuenta.

## Cambiar el modo desde la CLI {#change-mode-cli}

Si elegiste el modo equivocado en la instalación y no puedes acceder a la app en absoluto (p. ej. elegiste *localhost* en un servidor sin pantalla), edita `deploy/network.env` directamente y reinicia:

```bash
cd deploy
# make it reachable on your LAN over plain HTTP (mode 2):
#   OTW_BIND=0.0.0.0     (was 127.0.0.1)
#   OTW_HTTP_PORT=5454   (or your chosen port)
$EDITOR network.env
docker compose --env-file .env --env-file network.env up -d
```

`network.env` no contiene secretos: guarda la interfaz de enlace (`127.0.0.1` = solo esta máquina, `0.0.0.0` = todas las interfaces) y los puertos, interpolados por Compose. LAN + HTTPS necesita más de una línea (certificado + token de DNS), así que configúralo desde Ajustes o con `./setup.sh` modo `3`. Cuando puedas abrir la app, usa **Ajustes → Red**.
