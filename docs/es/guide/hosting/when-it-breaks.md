# Cuando algo falla

Los problemas con los que la gente realmente se encuentra, en el orden en que suelen ocurrir.
Para cada uno: qué ves, por qué y qué hacer.

::: tip Lo primero que debes probar, siempre
Si la instalación se detuvo, **ejecuta de nuevo la misma línea**.
Recuerda lo que ya funcionó y continúa desde donde se detuvo.
Nunca hace la misma pregunta dos veces.
:::

## Durante la instalación

### "The name … does not lead anywhere yet"

**Por qué:** tu dirección (por ejemplo `app.example.com`) aún no está conectada a tu máquina.

1. Inicia sesión en la empresa donde compraste la dirección.
2. Abre su página de **DNS** (a veces llamada **Zona** o **Registros DNS**).
3. Añade un registro con exactamente lo que imprimió el mensaje:

   | Tipo | Nombre | Valor |
   |---|---|---|
   | `A` | la palabra que muestra el mensaje (`@` para la dirección sin subdominio) | los números que muestra el mensaje |

4. Guarda.
5. Espera cinco minutos.
6. Ejecuta de nuevo la misma línea.

::: details El campo "Nombre" es el error habitual
Para `example.com`, escribe `@` (algunos sitios quieren el campo vacío).
Para `app.example.com`, escribe solo `app`, no la dirección completa.
:::

### "The name … does not lead to this machine"

**Por qué:** la dirección apunta a otro sitio: una máquina antigua, o una página de aparcamiento de la empresa que la vendió.

1. Abre la misma página de **DNS**.
2. Elimina todos los demás registros `A` con ese nombre.
3. Conserva solo el que tiene el valor que imprimió el mensaje.
4. Espera cinco minutos y luego ejecuta de nuevo la misma línea.

Si lo cambiaste hace solo unos minutos, responde **sí** cuando el instalador ofrezca esperar.
Comprueba cada 20 segundos durante un máximo de 10 minutos.

### "The address https://… is not answering yet"

**Por qué:** casi siempre una de dos cosas.

- Apuntaste la dirección a la máquina hace solo unos minutos. **Espera diez minutos** y luego ejecuta de nuevo la misma línea.
- Tu proveedor tiene su propio firewall delante de la máquina, y está cerrado.

Para abrir el firewall del proveedor:

1. Abre el panel de control de tu proveedor.
2. Busca los ajustes de **Firewall** o **Seguridad** de la máquina.
3. Permite el tráfico entrante **TCP 80** y **TCP 443**.
4. Ejecuta de nuevo la misma línea.

### "Something on this machine is already answering on port 80 / 443"

**Por qué:** tu proveedor instaló un servidor web en la máquina por ti. Ocupa el lugar que necesita OpenTraderWorld.

Pega esto y luego ejecuta de nuevo la misma línea:

```bash
systemctl disable --now apache2 nginx caddy 2>/dev/null; true
```

### "This machine has … MB of memory" o "Only … MB of disk space is free"

**Por qué:** la máquina es demasiado pequeña. OpenTraderWorld necesita unos **2 GB de memoria** y **8 GB de disco libre**.

1. En tu proveedor, cambia la máquina a un plan mayor.
2. Ejecuta de nuevo la misma línea.

### "This installer only knows Ubuntu and Debian"

**Por qué:** la máquina se creó con otro sistema.

1. En tu proveedor, **reinstala** (o **reconstruye**) la máquina con **Ubuntu 24.04** o **Debian 13**.
2. Ejecuta de nuevo la misma línea.

### "This needs the machine's administrator rights"

**Por qué:** has iniciado sesión con una cuenta que no tiene permiso para instalar software.

Ejecuta de nuevo la línea con `sudo` en medio, exactamente como muestra el mensaje:

```bash
curl -fsSL https://get.opentraderworld.com/configure_install.sh | sudo bash -s -- --domain app.example.com
```

## Después de la instalación

### La página ya no se abre

1. Inicia sesión en tu máquina.
2. Escribe:

   ```bash
   otw status
   ```

3. Si dice **Nothing is running** o **is not answering**, escribe:

   ```bash
   otw restart
   ```

4. Espera un minuto y recarga la página.

### Ya no puedo iniciar sesión en la propia máquina

**Por qué:** el instalador endureció la forma en que la máquina deja entrar a la gente.

- Si elegiste **la clave**: inicia sesión desde el mismo ordenador que usaste el día de la instalación. Las contraseñas se rechazan a propósito.
- Si elegiste **una contraseña**: inicia sesión con el nombre de cuenta que aparece en tu tarjeta, **no** `root`. El sign-in directo como `root` se rechaza a propósito.

¿Perdiste ese ordenador o esa contraseña? Usa la **consola** (a veces llamada **VNC**, **rescate** o **terminal web**) en el panel de control de tu proveedor. Funciona incluso cuando la vía normal de entrada está cerrada.

### Olvidé la contraseña de OpenTraderWorld

1. Inicia sesión en tu máquina.
2. Escribe (sustituye `admin` por tu nombre de usuario si lo cambiaste):

   ```bash
   docker exec -it opentraderworld-core-1 /app/otw-core reset-password admin
   ```

3. Inicia sesión en la app con la contraseña que imprime. La app te pide que elijas una nueva.

Para ver de nuevo tu dirección y nombre de usuario, escribe `otw card`.

### El navegador muestra "No seguro" o rechaza la página

- Escribe la dirección con `https://` delante.
- Si empezó unos minutos después de la instalación, espera diez minutos: el candado aún se está emitiendo.
- En una instalación doméstica (no un servidor alquilado), consulta [Solución de problemas](/es/guide/troubleshooting).

### La máquina está llena

**Por qué:** las copias de seguridad nocturnas y el historial de precios descargado ocupan espacio con el tiempo.

1. Escribe `otw status` para ver cuánto espacio queda.
2. En tu proveedor, dale a la máquina un disco mayor.
3. Escribe `otw restart`.

## ¿Sigues atascado?

1. Inicia sesión en tu máquina.
2. Escribe:

   ```bash
   otw report
   ```

3. Escribe un archivo e imprime dónde está. El archivo no contiene ninguna contraseña.
4. Abre un issue en [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues), cuenta qué estabas haciendo y adjunta ese archivo.
