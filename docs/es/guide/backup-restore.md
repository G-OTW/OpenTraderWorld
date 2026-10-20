# Copia y restauración

Todo vive en una única base de datos PostgreSQL, así que una copia es un solo `pg_dump`. **Ajustes → Copia y restauración** en la app muestra estos comandos ya rellenados para tu despliegue. Ejecútalos en el host donde está desplegada la pila; usan el contenedor de Postgres existente, sin necesidad de acceso adicional.

La sección tiene dos pestañas, cada una dividida en **Copia** y **Restauración**:

- **Completa**: toda la base de datos, hecha en el host, para cuando la máquina muere.
- **Parcial**: los módulos que marques, en un solo zip, para mudarte o conservar una copia legible.

## Copia completa

Volcado simple:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld > otw-backup-$(date +%F).sql
```

### Cífrala (recomendado)

Un volcado contiene tus datos en claro. Pásalo por `gpg` (o `age`) para que el archivo quede cifrado en disco. Se te pedirá una frase de contraseña:

```bash
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld | gpg -c --cipher-algo AES256 -o otw-backup-$(date +%F).sql.gpg
```

## Notas de seguridad

- Las **claves API y credenciales de proveedores** (feeds de noticias, proveedores de datos de mercado) ya están cifradas en reposo con `OTW_SECRET_KEY`, por lo que aparecen solo como texto cifrado en el volcado.
- Haz copia de **`OTW_SECRET_KEY`** (de `deploy/.env`) **por separado**, no dentro del mismo volcado, o esos secretos cifrados no podrán restaurarse.
- El volcado incluye **tokens de sesión** activos. Trata el archivo como un secreto, o elimina la tabla `sessions` tras restaurar e inicia sesión de nuevo.
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

En una base de datos nueva y vacía (una pila recién creada):

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld < otw-backup-2026-07-06.sql
```

Desde una copia cifrada:

```bash
gpg -d otw-backup-2026-07-06.sql.gpg | \
  docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld
```

Asegúrate de que la pila restaurada use la **misma `OTW_SECRET_KEY`** que cuando se hizo la copia, o las credenciales de proveedor almacenadas serán ilegibles.
