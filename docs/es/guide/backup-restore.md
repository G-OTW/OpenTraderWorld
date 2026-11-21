# Copia y restauración

Todo vive en una única base de datos PostgreSQL, así que una copia es un solo `pg_dump`. **Ajustes → Copia y restauración** en la app muestra los mismos comandos. Ejecútalos en el host donde está desplegada la pila, desde el directorio de instalación (el que contiene `deploy/`); usan el contenedor de Postgres existente, sin necesidad de acceso adicional.

La sección tiene dos pestañas, cada una dividida en **Copia** y **Restauración**:

- **Completa**: toda la base de datos, hecha en el host, para cuando la máquina muere.
- **Parcial**: los módulos que marques, en un solo zip, para mudarte o conservar una copia legible.

## Copia completa

Volcado simple:

```bash
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner > otw-backup-$(date +%F).sql
```

### Cífrala (recomendado)

Un volcado contiene tus datos en claro. Pásalo por `gpg` (o `age`) para que el archivo quede cifrado en disco. Se te pedirá una frase de contraseña; `GPG_TTY` y `--pinentry-mode loopback` permiten que esa petición llegue a tu terminal mientras el volcado llena la entrada de gpg:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner \
  | gpg --symmetric --cipher-algo AES256 --pinentry-mode loopback \
  > otw-backup-$(date +%F).sql.gpg
```

### Copia automática {#automatic-backup}

Cuando un script de copia en el host informa a la app (el [instalador de servidor](#server-installs) configura uno), la pestaña **Completa** se abre con una tarjeta **Copia automática**: hace cuánto se hizo la última copia, adónde fue, si se comprobó, la programación y el tamaño. Pasa a advertencia cuando la última ejecución falló, o tras 48 horas sin ninguna. Sin un script así no hay tarjeta, y los comandos de arriba son todo lo que hay.

## Notas de seguridad

- Las **claves API y credenciales de proveedores** (feeds de noticias, proveedores de datos de mercado) ya están cifradas en reposo con `OTW_SECRET_KEY`, por lo que aparecen solo como texto cifrado en el volcado.
- Haz copia de **`OTW_SECRET_KEY`** (de `deploy/.env`) **por separado**, no dentro del mismo volcado, o esos secretos cifrados no podrán restaurarse.
- El volcado incluye **tokens de sesión** activos. Trata el archivo como un secreto, o vacía la tabla `sessions` tras restaurar (`DELETE FROM sessions;`) e inicia sesión de nuevo. No elimines la tabla: el inicio de sesión la necesita.
- Guarda la copia cifrada **fuera de la máquina** y rota las copias antiguas.

## Parcial (por módulo)

La pestaña **Parcial** toma los módulos que marques y te da **un único archivo zip**, para mover un journal a otra instancia o conservar una copia legible. Se ejecuta con la app en marcha, a diferencia de la copia completa.

### Copia parcial

1. Abre **Ajustes → Copia y restauración → Parcial → Copia**.
2. Marca los módulos que quieras. Cada uno muestra su número de filas y su tamaño, el conjunto marcado se totaliza bajo la lista, y *Qué hay en la selección* lo desglosa tabla por tabla. Las barras históricas empiezan sin marcar: son con diferencia la tabla más grande, y pueden volver a descargarse de tu proveedor.
3. Deja desactivado **Incluir credenciales de proveedor almacenadas** salvo que sepas por qué las necesitas. Esos valores están cifrados con la `OTW_SECRET_KEY` de esta instancia y son ilegibles en cualquier otro sitio.
4. Haz clic en **Descargar datos seleccionados**. Obtendrás `otw-data-YYYY-MM-DD.zip`.

Dentro del zip: `manifest.json` (qué contiene, qué versión lo escribió) y un `tables/<name>.jsonl` por tabla, un objeto JSON por fila. Cualquier herramienta puede leerlo.

### Restauración parcial

1. Abre **Ajustes → Copia y restauración → Parcial → Restauración** en la instancia de destino y elige el archivo.
2. El archivo se lee en cuanto lo eliges, y no se escribe nada: obtienes la versión que lo escribió y, por módulo y por tabla, cuántas filas trae frente a cuántas hay ahora. Una tabla muy grande se informa como estimación, marcada con `~`. Un archivo que esta instancia rechazaría (dañado, o de una versión más reciente) se rechaza en este punto, antes de que te comprometas a nada.
3. Elige cómo debe encontrarse con los datos que ya hay:
   - **Añadir lo que falta** conserva todo lo presente y añade solo las filas que aún no están. No se sobrescribe nada.
   - **Reemplazar** borra los datos de todos los módulos del archivo y luego carga la versión del archivo. Te pide escribir `REPLACE`, y antes guarda una copia de los datos actuales.

   La línea bajo la opción convierte esos recuentos en lo que va a ocurrir. Reemplazar es exacto: *borra las N filas de aquí, pone en su lugar las M filas del archivo*. Fusionar solo puede dar un techo, *añade hasta M filas*: una fila cuya clave ya existe aquí se omite, y solo la propia carga sabe cuántas son.
4. Haz clic en **Cargar este archivo**.

Todo ocurre en una sola transacción: si cualquier parte falla, no se cambia nada.

::: warning Un archivo de una versión más reciente se rechaza
Cargar un paquete escrito por una versión más reciente se rechaza en lugar de intentarse. Actualiza primero la instancia y luego cárgalo.
:::

Las filas conservan sus identificadores originales y el archivo entero se lee en memoria, así que una selección muy grande (normalmente las barras históricas) se rechaza con un mensaje que remite a la copia con `pg_dump` de arriba. Esa es la herramienta adecuada para "todo, incluido lo que nunca miro".

## Restauración completa

Un volcado solo se restaura en una base de datos vacía. El core ejecuta sus migraciones en cada arranque, así que una base de datos sobre la que ya arrancó (cualquier pila en marcha, incluida una que `setup.sh` acaba de crear) choca con el volcado. Detén el core, sustituye la base de datos por una vacía, carga el volcado y vuelve a arrancar el core:

```bash
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q < otw-backup-YYYY-MM-DD.sql
docker compose -f deploy/docker-compose.yml start core
```

La línea `DROP DATABASE` borra lo que la pila contiene ahora: haz antes un volcado si te importa. `ON_ERROR_STOP=1` se detiene en el primer error en lugar de cargar medio volcado.

Desde una copia cifrada:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
gpg -d otw-backup-YYYY-MM-DD.sql.gpg \
  | docker compose -f deploy/docker-compose.yml exec -T postgres \
    psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q
docker compose -f deploy/docker-compose.yml start core
```

Asegúrate de que la pila restaurada use la **misma `OTW_SECRET_KEY`** que cuando se hizo la copia, o las credenciales de proveedor almacenadas serán ilegibles.

## Instalaciones en servidor {#server-installs}

Una máquina configurada con el [instalador de servidor](/es/guide/hosting/when-it-breaks#installer-options) tiene el comando `otw`, que hace todo lo anterior por ti:

| Comando | Qué hace |
|---|---|
| `otw backup` | Hace ahora una copia en `/var/backups/otw` y comprueba que el archivo está completo. |
| `otw restore` | Lista las copias de la máquina, pregunta cuál y la vuelve a poner cuando escribes `RESTORE`. Antes hace una copia completa de los datos actuales, y luego reinicia el core. |

El instalador también programa `otw backup` cada noche hacia las 03:20, que es lo que enciende la tarjeta [Copia automática](#automatic-backup). Una copia nocturna deja fuera las dos tablas que se pueden volver a descargar (el catálogo de instrumentos y las barras históricas); la del domingo es completa. Se conservan las 10 últimas copias nocturnas y las 2 últimas completas. Se quedan en la máquina: cópialas a otro sitio para sobrevivir a un servidor muerto.
