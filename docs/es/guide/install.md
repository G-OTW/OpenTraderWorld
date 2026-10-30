# Instalación

Autoaloja OpenTraderWorld en tu propia máquina o servidor. Lleva unos 5 minutos.

::: info Solo en contenedores (por ahora)
OpenTraderWorld se ejecuta como una pila de Docker Compose, el único despliegue admitido. Una instalación nativa es posible pero no se recomienda: Docker mantiene la instalación poco intrusiva (todo vive en contenedores y volúmenes) y rápida de reconstruir. Consulta [Obtener Docker](/es/guide/docker) para saber por qué y para los pasos de instalación por sistema operativo.
:::

## Requisitos

- **Docker** con Docker Compose. ¿No lo tienes? [Obtener Docker](/es/guide/docker) cubre macOS, Windows y Linux en pocos comandos.
- Linux, macOS o Windows.
- Un puerto libre (**5454** por defecto; puertos **80** + **443** para los modos HTTPS). Puedes cambiarlo durante la configuración. Los modos localhost y LAN por HTTP plano también ocupan el **8443** en `127.0.0.1`.

Comprueba que Docker está listo:

```bash
docker --version
docker compose version
```

## Instalación con un solo comando (recomendada)

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

El instalador comprueba que Docker está listo, descarga los archivos de despliegue (solo el directorio `deploy/`, sin código fuente ni toolchain) en `./opentraderworld` y pasa a la configuración guiada que sigue, que **descarga las imágenes precompiladas** de Docker Hub.

Las opciones van después de `bash -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash -s -- --dir ~/otw
```

| Opción | Por defecto | Notas |
|---|---|---|
| `--dir <path>` | `./opentraderworld` | Directorio de instalación. Rechaza un directorio no vacío o una instalación existente. |
| `--ref <ref>` | `master` | Rama o tag a instalar. |
| `--build` | off | Clona todo el código fuente y compila las imágenes localmente en lugar de descargarlas (requiere `git` y el toolchain). |
| `--yes` | off | Se pasa a `setup.sh`: toma todos los valores por defecto cuando no hay terminal en la que preguntar. |
| `--wipe-volumes` | off | Se pasa a `setup.sh`: elimina sin preguntar los volúmenes de una instalación anterior. **Esto borra los datos de esa instalación.** |

`OTW_DIR` y `OTW_REF` fijan los mismos valores que `--dir` y `--ref` (una opción prevalece sobre su variable), y `OTW_REPO` el repositorio de GitHub (por defecto `G-OTW/OpenTraderWorld`).

## Desde un clon de git (alternativa)

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

## La configuración guiada

Las dos vías anteriores ejecutan `deploy/setup.sh`. Hace unas preguntas, genera secretos robustos, escribe la configuración y lo arranca todo.

Pregunta por:

| Pregunta | Por defecto | Notas |
|---|---|---|
| **Modo de red** | `1` (localhost) | `1` solo esta máquina · `2` LAN por HTTP plano · `3` LAN por HTTPS con un certificado real · `4` internet público en tu propio dominio. Se puede cambiar después, consulta [Red y acceso remoto](/es/config/network). |
| **Usuario administrador** | `admin` | La cuenta de administrador se crea por ti; se genera una contraseña robusta y se muestra **una sola vez**. |
| **Puerto HTTP** | `5454` | Solo modos 1 y 2; los modos 3 y 4 usan 80 + 443. |
| **Dominio DuckDNS / token / IP de la LAN** | ninguno | Solo modo 3. |
| **Dominio público** | ninguno | Solo modo 4, y ya debe resolver a este servidor. |
| **Nivel de registro** | `info` | `trace` / `debug` / `info` / `warn` / `error`. |

También acepta estas opciones:

| Opción | Notas |
|---|---|
| `--build` | Compila las imágenes desde el código fuente en lugar de descargarlas (ver más abajo). |
| `--yes` | Toma todos los valores por defecto cuando no hay terminal en la que preguntar. |
| `--wipe-volumes` | Elimina sin preguntar los volúmenes de una instalación anterior. **Esto borra los datos de esa instalación.** |
| `--non-interactive` | Igual que `OTW_NONINTERACTIVE=1`: lee cada respuesta del entorno, nunca de una terminal. Una variable sin definir toma el valor por defecto. |

Las variables no interactivas, una por pregunta: `NET_CHOICE` (`1` a `4`), `OTW_HTTP_PORT`, `ADMIN_USER`, `OTW_ADMIN_PASSWORD` (generada si no se define), `OTW_LOG`, `DUCK_DOMAIN`, `DUCK_TOKEN` y `LAN_IP` (modo 3), `OTW_DOMAIN` (modo 4), y `OVERWRITE`, `CLEAN_VOLS`, `WIPE_VOL`, `START_NOW` (`y` o `N`) para las confirmaciones.

Los **secretos de la base de datos y de sesión se generan automáticamente**, así que nunca los escribes. Se guardan en `deploy/.env` (permisos de archivo `600`, nunca se suben a git).

Al final, deja que el script arranque la pila: espera a la API, **crea tu cuenta de administrador** y muestra la contraseña generada **una sola vez**, así que cópiala antes de cerrar la terminal.

Por defecto la configuración **descarga las imágenes precompiladas** de Docker Hub: sin toolchain de Rust ni Node, primer arranque en un par de minutos. Ejecuta `./setup.sh --build` para compilar los tres servicios desde el código fuente (desarrollo, cambios locales).

::: tip Servidores sin pantalla
El administrador lo crea el propio core en el primer arranque (a partir de `deploy/.env`), así que no necesitas un navegador en el servidor. Anota la contraseña mostrada e inicia sesión desde cualquier máquina que alcance la app.
:::

::: warning Reinstalar sobre datos anteriores
Si existen volúmenes de Docker de una instalación anterior, la configuración ofrece borrarlos. El valor por defecto es **No** en todas partes: borrar (y perder la base de datos anterior) solo ocurre con un `y` explícito. Unos secretos nuevos sobre un volumen de base de datos antiguo no pueden funcionar, por lo que rechazar aborta la configuración en lugar de arrancar una pila rota.
:::

## Instalación manual (alternativa)

Si prefieres configurar a mano:

```bash
cd deploy
cp .env.example .env
```

Edita `.env` y define al menos:

- `POSTGRES_PASSWORD`: una contraseña robusta
- `DATABASE_URL`: debe contener la misma contraseña, p. ej. `postgres://otw:YOUR_PASSWORD@postgres:5432/opentraderworld`
- `SESSION_SECRET`: una cadena aleatoria larga
- `OTW_SECRET_KEY`: otra cadena aleatoria larga, la clave que cifra las credenciales almacenadas. Mantenla estable: cambiarla después las vuelve ilegibles.

Compose también necesita los dos archivos de red que habría escrito `setup.sh`. Para el modo localhost en el puerto 5454:

```bash
cat > network.env <<'EOF'
OTW_BIND=127.0.0.1
OTW_HTTPS_BIND=127.0.0.1
OTW_HTTP_PORT=5454
OTW_HTTPS_PORT=8443
OTW_DOMAIN=:80
EOF
printf 'OTW_ACME_GLOBAL=\n' > dns.env
```

Ambos se reescriben después desde **Ajustes → Red**, consulta [Red y acceso remoto](/es/config/network).

Luego arranca la pila. Por defecto esto **descarga las imágenes precompiladas** de Docker Hub (no hace falta toolchain de Rust ni Node):

```bash
docker compose -f docker-compose.yml -f docker-compose.images.yml \
  --env-file .env --env-file network.env up -d
```

::: details Compilar desde el código fuente
Para desarrollo, o para ejecutar cambios locales, omite el override de imágenes y compila tú mismo los tres servicios (requiere el toolchain; la compilación de Rust es lenta):

```bash
docker compose --env-file .env --env-file network.env up --build -d
```
:::

## Crear el administrador (solo instalaciones manuales)

Si usaste `./setup.sh` y dejaste que arrancara la pila, **tu administrador ya existe**, así que sáltate esto.

En caso contrario, abre la app en tu navegador (`http://localhost:5454` por defecto). En la primera visita, OpenTraderWorld detecta que aún no hay administrador y muestra el **asistente de configuración**: elige un usuario y una contraseña, envíalo y llegarás al dashboard. La contraseña necesita al menos 12 caracteres, y se rechaza un carácter repetido, una secuencia contada, una que contenga el nombre de usuario o una de una lista de filtraciones conocidas. Las contraseñas se almacenan con hash argon2.

En los modos HTTPS (LAN + HTTPS, público) el asistente también pide un **token de configuración**, para que nadie que simplemente encuentre la dirección pueda reclamar la cuenta antes que tú. Core lo imprime en su registro al arrancar: `docker compose logs core`.

::: details Crear el administrador desde la CLI (sin pantalla, sin navegador)
Llama al endpoint de primer arranque desde dentro de la pila: funciona sea cual sea tu interfaz de enlace, y se rechaza (HTTP 409) si ya existe un administrador. En los modos HTTPS, añade el token de configuración del registro de core como una cabecera más, `--header=X-OTW-Setup-Token:<token>`, o responde 403 `setup_token_required`:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T caddy \
  wget -qO- --header=Content-Type:application/json \
  --post-data='{"username":"admin","password":"CHOOSE-A-STRONG-ONE"}' \
  http://core:8080/api/setup
```
:::

## Verificar que está en marcha

- App: `http://localhost:5454` (o el puerto/dominio que elegiste)
- Comprobación de estado: `http://localhost:5454/api/health` → `{"status":"ok"}` (con la sesión iniciada, también devuelve la versión, el tiempo en marcha y el estado de cada servicio)

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Operaciones del día a día

Ejecuta esto desde `deploy/`. Cada comando lleva los dos archivos de entorno, y una instalación por imágenes lleva además el override de imágenes, así que parte de la línea que corresponde a tu instalación:

```bash
# image install (the default)
docker compose -f docker-compose.yml -f docker-compose.images.yml --env-file .env --env-file network.env
# source build
docker compose --env-file .env --env-file network.env
```

| Acción | Añade |
|---|---|
| Iniciar | `up -d` |
| Detener | `down` |
| Ver registros | `logs -f` |
| Recompilar tras cambiar código (compilación desde fuente) | `up --build -d` |
| Detener **y borrar todos los datos** | `down -v` |

Para pasar a una versión más reciente, sigue [Actualizar](/es/guide/updating): los tags de las imágenes están fijados en `docker-compose.images.yml`, así que un simple `pull` descarga la versión que ya ejecutas.

Tus datos viven en volúmenes con nombre de Docker y **persisten** entre `up`/`down`. Solo se borran con `down -v`.

## Próximos pasos

- [Primeros pasos](/es/guide/first-steps): inicia sesión, define valores predeterminados, instala módulos.
- [Red y acceso remoto](/es/config/network): accede a la app desde otros dispositivos, HTTPS en LAN, exposición pública.
- ¿Algo va mal? Consulta [Solución de problemas](/es/guide/troubleshooting).
