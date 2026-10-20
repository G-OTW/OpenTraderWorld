# Actualización

OpenTraderWorld te avisa cuando hay una nueva versión disponible en **Ajustes → Actualizar app** (consulta GitHub). La app **no puede actualizarse a sí misma por diseño** (se ejecuta sin acceso a la shell ni a Docker, para reducir la superficie de ataque), así que actualizar son un par de comandos en el host.

## Antes de actualizar

1. **Haz una copia de seguridad de la base de datos**: consulta [Copia y restauración](/es/guide/backup-restore).
2. Echa un vistazo a las notas de la versión por si hay cambios incompatibles.

## ¿Qué instalación tienes?

Mira tu directorio de instalación:

- Solo `deploy/` dentro, sin `.git`: **instalación con imágenes** (el instalador de un solo comando,
  o `setup.sh` sin `--build`). Es la opción por defecto.
- `core/`, `frontend/` y un `.git`: **compilación desde el código fuente** (`install.sh --build`, o un clon
  más `./setup.sh --build`).

## Instalación con imágenes

No se compila nada y no hay checkout de git, así que actualiza `deploy/` desde la versión publicada
(ahí es donde se fijan las nuevas etiquetas de imagen), y luego descarga y reinicia:

```bash
cd /path/to/opentraderworld
TMP=$(mktemp -d)
curl -fsSL https://codeload.github.com/G-OTW/OpenTraderWorld/tar.gz/master | tar -xz -C "$TMP"
cp -R "$TMP"/*/deploy/. deploy/
rm -rf "$TMP"
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  pull
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d
```

Tu configuración se conserva: `.env`, `network.env` y `dns.env` no forman parte de la
versión, así que la copia nunca los sobrescribe. Todo lo demás en `deploy/` se sustituye por
la nueva versión, que es justo lo que se busca: las ediciones locales de `docker-compose.yml` o `Caddyfile`
se pierden, guárdalas en un parche si las necesitas.

## Compilación desde el código fuente

Actualiza el checkout y luego recompila:

```bash
cd /path/to/OpenTraderWorld
git fetch origin
git reset --hard origin/master
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d --build
```

::: warning Usa `git reset --hard`, no `git pull`
Cada versión se publica como una instantánea nueva del repositorio, así que `git pull` informa de
"divergent branches" y falla. `git reset --hard origin/master` hace que tu checkout
coincida exactamente con la nueva versión. Tus datos y tu configuración no se tocan: viven en
volúmenes de Docker y en `.env` / `network.env`, que git no rastrea. Si editaste
archivos rastreados localmente, guárdalos antes con stash (`git stash`).
:::

Para descargar las imágenes publicadas en lugar de recompilar desde tu checkout, añade
`-f deploy/docker-compose.images.yml` y usa `pull` + `up -d` como en la instalación con imágenes.

Eso es todo:

- Los contenedores se recrean con la nueva versión y se reinician.
- **Las migraciones de la base de datos se ejecutan automáticamente** en el primer arranque del nuevo contenedor core.
- Tus datos no se tocan: viven en volúmenes de Docker, independientes de las imágenes.

La app está fuera de línea brevemente mientras se recrean los contenedores. Los comandos exactos (con tus rutas configuradas) también se muestran en **Ajustes → Actualizar app**.

## Puntual: OAuth para AI agents (0.0.16) {#oauth-caddyfile}

El sign-in OAuth para clientes MCP necesita que los documentos de descubrimiento en `/.well-known/oauth-*`
lleguen al core. El procedimiento de instalación con imágenes anterior y las compilaciones desde fuente traen el nuevo
`deploy/Caddyfile`, igual que las instalaciones nuevas. Una instalación actualizada con `otw update` conserva
su Caddyfile antiguo: todo funciona salvo OAuth hasta que añadas este bloque, justo antes de la
línea `# Everything else → static frontend`:

```
	handle /.well-known/oauth-* {
		header Content-Security-Policy "default-src 'none'; frame-ancestors 'none'"
		reverse_proxy core:8080
	}
```

Luego recrea Caddy (una recarga no basta: la mayoría de los editores sustituyen el archivo, y el
contenedor conserva el antiguo):

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d --force-recreate caddy
```

## Puntual: retirar la contraseña de arranque {#retire-bootstrap-password}

Si el core registra esto al arrancar, te está hablando a ti:

```
OTW_ADMIN_PASSWORD is still set but the admin account already exists.
```

`setup.sh` crea el administrador a partir de `deploy/.env` en el primer arranque y luego vacía esa línea. En
una instalación hecha antes de que lo hiciera, el valor sigue en el archivo, y en el
entorno del contenedor, donde `docker inspect` se lo entrega a cualquiera que alcance el demonio de
Docker. En este punto no concede nada, simplemente es una copia más de una contraseña en disco.

```bash
sed -i.bak 's/^OTW_ADMIN_PASSWORD=.*/OTW_ADMIN_PASSWORD=/' deploy/.env
chmod 600 deploy/.env && rm -f deploy/.env.bak
```

Luego recrea el core para que también desaparezca del entorno:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d core
```

Conserva `OTW_ADMIN_USER`: una contraseña vacía convierte todo el arranque en una operación sin efecto, que es lo que
quieres, y la línea sigue documentando cómo una instalación sin pantalla crea su primera cuenta.
