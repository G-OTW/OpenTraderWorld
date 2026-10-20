# Control por voz

Maneja OpenTraderWorld con tu voz, o dicta en cualquier campo de texto. **Solo pulsar para hablar**: el micrófono se abre al pulsar y se cierra al soltar. Nada escucha entre medias.

**Está desactivado por defecto.** Actívalo en **Ajustes → Voz y atajo**.

## El micrófono necesita HTTPS o localhost {#https}

Los navegadores solo dan acceso al micrófono (y a su reconocimiento de voz integrado) a una página web en un **origen seguro**: una página servida por **HTTPS**, o abierta en **localhost**. Es una regla del navegador, no de OpenTraderWorld, y ningún ajuste de la app puede levantarla.

| Cómo abres OTW | ¿Funciona la voz? |
|---|---|
| En la máquina que la ejecuta, `http://127.0.0.1:5454` o `http://localhost:5454` | Sí |
| Modo [LAN + HTTPS](/es/config/network#lan-https) o [Público](/es/config/network#public) | Sí |
| Modo [Red local (LAN)](/es/config/network) por HTTP plano, desde otro dispositivo | **No**, el navegador oculta el micrófono |

Para usar la voz desde un móvil u otro ordenador de tu red, cambia **Ajustes → Red** a **LAN + HTTPS**. En HTTP plano, la página de ajustes de voz muestra un aviso y el botón del micrófono explica por qué no puede iniciarse.

## Dos atajos, dos modos

| | Por defecto | Qué ocurre con lo que dices |
|---|---|---|
| **Comando** | `Alt+V` (`⌥V` en Mac) | Se convierte en un plan de acciones, incluso cuando un campo de texto tiene el foco. |
| **Dictado** | `Alt+Shift+V` (`⌥⇧V`) | Se escribe, palabra por palabra, en el campo de texto que tiene el foco. Nunca se interpreta como comando. |

Mantén pulsado el atajo mientras hablas, suéltalo para terminar, `Esc` para cancelar. El **micrófono de la barra superior** siempre ejecuta comandos: haz clic para empezar, otro clic para parar, o mantenlo pulsado como un walkie-talkie.

Ambos atajos pueden cambiarse en **Ajustes → Voz y atajo**. Cada uno necesita `Ctrl`, `Alt` o `⌘` (o una tecla de función `F1` a `F12`), para que nunca estorbe al escribir normal, y los dos deben ser distintos.

## Motor de voz

El motor convierte tu grabación en texto. Elige uno en **Ajustes → Voz y atajo**.

| Motor | Configuración | Adónde va el audio |
|---|---|---|
| **Este navegador** | ninguna | El servicio de voz del fabricante del navegador (Google para Chrome, Microsoft para Edge, Apple para Safari). No disponible en Firefox. |
| **Whisper autoalojado** | el servicio incluido, consulta [Ejemplo: Whisper autoalojado](#whisper-example) | Se queda en tu máquina |
| **Servidor whisper.cpp** | tu propio servidor, su endpoint `/inference` | Se queda en tu servidor |
| **OpenAI / Groq** | una clave API (pegada o enlazada desde la [Bóveda](/es/config/settings#vault)) | Se envía a ese proveedor |
| **Otro** | cualquier servidor que exponga la API `/audio/transcriptions` de OpenAI | Ese servidor |

Con un motor de servidor, el navegador graba, convierte el audio a un pequeño archivo WAV y OTW lo retransmite al motor. **Probar** envía medio segundo de silencio para comprobar la URL, la clave y el modelo. Las claves se cifran en reposo y nunca se devuelven al navegador.

### Ejemplo: Whisper autoalojado, desde cero {#whisper-example}

El servicio Whisper incluido es opcional y no se inicia con un simple `up`. Los pasos 1 a 3 se hacen una vez: el modelo se conserva en el volumen `whisper-cache` entre reinicios.

**1. Inicia el servicio**, desde la raíz del repositorio:

```bash
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --env-file deploy/network.env --profile voice up -d whisper
docker logs -f opentraderworld-whisper-1
```

Espera a `Uvicorn running on http://0.0.0.0:8000` y luego `Ctrl+C`. El puerto solo es accesible para OTW dentro de Docker, no desde tu navegador, y eso es lo previsto.

**2. Descarga el modelo** (unos 500 MB, unos minutos):

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen(urllib.request.Request('http://localhost:8000/v1/models/Systran/faster-whisper-small',method='POST'),timeout=1800).read())"
```

**3. Comprueba que está instalado**: la respuesta debe listar `Systran/faster-whisper-small`.

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen('http://localhost:8000/v1/models').read())"
```

**4. Conéctalo a OTW.** Abre la app en `http://localhost:5454` (o por HTTPS, consulta [arriba](#https)), luego **Ajustes → Voz y atajo**:

1. **Añadir motor**, preajuste **Whisper autoalojado**: rellena `http://whisper:8000/v1` y `Systran/faster-whisper-small`.
2. **Guardar y probar**: el motor muestra **Funciona · … ms**.
3. Selecciona el motor y luego pon el interruptor de arriba en **Activado**. Opcionalmente define el **Idioma**.

**5. Pruébalo**: mantén `⌥V` / `Alt+V`, di *"abre journal"*, suelta y pulsa `Enter`. Para dictar, haz clic en un campo de texto y mantén `⌥⇧V` / `Alt+Shift+V`.

La primera transcripción tras un reinicio es más lenta mientras el modelo se carga en memoria.

## Comandos de voz

**Ajustes → Voz y atajo → Comandos de voz**: un comando es una **frase** (más otras formas de decirla) y una lista ordenada de **pasos**:

- **Abrir una página**: un módulo, el dashboard o Ajustes.
- **Preguntar al agent**: un prompt enviado al asistente flotante, que actúa con sus propias herramientas.
- **Ejecutar un workflow**: inicia un workflow de [Automator](/es/modules/automator).
- **Tema**, **Ocultar cifras**: lo mismo que los botones de la barra superior.
- **Hablar**: lee una frase en voz alta.

Una frase solo se activa cuando se dice **sola**, nunca como una palabra dentro de una oración: dictar "una tortuga azul" no ejecuta tu comando *tortuga*.

### Incluido, sin configuración

- **"abre &lt;página&gt;"** (*"open &lt;page&gt;"*, *"ouvre &lt;page&gt;"*, *"öffne &lt;Seite&gt;"*, *"apri &lt;pagina&gt;"*, *"打开 &lt;页面&gt;"*) abre cualquier módulo instalado, el dashboard o Ajustes. Un nombre que coincide con varias páginas se rechaza mostrando la lista, nunca se adivina.
- Con **Pasar el resto al agent** activado, todo lo que ningún comando reconozca va al asistente como una sola petición.

### Encadenar

Di varias cosas de una vez, unidas por *y* o *luego* (*and*, *then* en inglés, *et*, *puis* en francés, etcétera, según el ajuste de **Idioma**). Una coma en la transcripción también separa:

> "tortuga, luego abre ajustes y compara AAPL y MSFT"

ejecuta tu comando *tortuga*, abre Ajustes y luego pide al agent "compara AAPL y MSFT". Los fragmentos sobrantes que quedan juntos se mantienen unidos, así el agent recibe la petición completa.

## Confirmación

Cada plan se **muestra antes de ejecutarse**: lo que se oyó, cada paso y de dónde viene. `Enter` lo ejecuta, `Esc` lo cancela. Los pasos se ejecutan en orden y el plan se detiene en el primer fallo, indicando el paso.

Un comando puede marcarse como **Ejecutar sin confirmación**. Está **desactivado por defecto**, y solo se aplica cuando esa frase se dice sola: encadenada con cualquier otra cosa, el plan se muestra igualmente primero. Los pasos del agent mantienen la confirmación propia del asistente para cada escritura.

## Conviene saber

- El asistente está oculto en la página **Agent**, así que un plan con un paso de agent no puede ejecutarse desde ahí.
- **Leer el resultado en voz alta** usa las voces del propio navegador: sin configuración, ningún audio sale del navegador.
- La demo pública muestra las páginas de voz en solo lectura: no guarda ajustes ni envía audio.
- **Tu propio proxy inverso** delante de OTW no debe bloquear el micrófono: su cabecera `Permissions-Policy` necesita `microphone=(self)`. Con `microphone=()` el navegador rechaza al instante, sin preguntar.
