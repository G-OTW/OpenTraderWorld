# Obtener Docker

OpenTraderWorld se distribuye como un conjunto de contenedores Docker, y actualmente esa es la **única forma admitida de ejecutarlo**. Una instalación nativa (ejecutar el core en Rust, PostgreSQL y el frontend directamente en el host) es posible si sabes lo que haces, pero **no se recomienda ni está documentada**. Docker se prioriza a propósito:

- **Poco intrusivo**: no se instala nada en tu sistema salvo el propio Docker. La app, la base de datos y el proxy viven en contenedores; tus datos viven en volúmenes con nombre. Eliminarlo todo es `docker compose down -v` más borrar la carpeta.
- **Idéntico en todas partes**: la misma pila se ejecuta sin cambios en macOS, Linux y Windows.
- **Rápido de actualizar**: actualiza el repositorio y descarga las nuevas imágenes (consulta [Actualización](/es/guide/updating)); un contenedor dañado se recrea en segundos sin tocar tus datos.

Si ya tienes Docker, ve directamente a [Instalación](/es/guide/install).

## macOS

Instala **Docker Desktop**:

- Descárgalo desde [docker.com](https://www.docker.com/products/docker-desktop/) (elige Apple Silicon o Intel), abre el `.dmg` y arrastra Docker a Aplicaciones, o con Homebrew:

  ```bash
  brew install --cask docker
  ```

- Abre **Docker** una vez desde Aplicaciones y deja que termine de arrancar (el icono de la ballena en la barra de menús deja de animarse).

Docker Compose está incluido.

## Windows

Instala **Docker Desktop** con el backend WSL 2:

1. Requisitos: Windows 10/11 de 64 bits con **WSL 2**, activado desde un PowerShell de administrador si hace falta: `wsl --install`, y luego reinicia.
2. Instala Docker Desktop desde [docker.com](https://www.docker.com/products/docker-desktop/), o:

   ```powershell
   winget install Docker.DockerDesktop
   ```

3. Abre Docker Desktop y mantén el ajuste predeterminado *Use WSL 2*.

Ejecuta los comandos de OpenTraderWorld desde cualquier terminal (PowerShell o una shell de WSL). Docker Compose está incluido.

## Linux

En un escritorio o en un servidor sin pantalla, instala **Docker Engine** (no hace falta Desktop). El script de conveniencia funciona en todas las distribuciones principales:

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER   # run docker without sudo
newgrp docker                    # or log out and back in
sudo systemctl enable --now docker
```

¿Prefieres los paquetes de tu distribución? Consulta las [instrucciones oficiales por distribución](https://docs.docker.com/engine/install/). Las instalaciones recientes de Engine incluyen el plugin de Compose.

## Verificar

```bash
docker --version
docker compose version
docker run --rm hello-world
```

Los tres funcionan → estás listo.

## Desplegar OpenTraderWorld

Un solo comando y luego sigue las indicaciones:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

El recorrido completo (qué significan las preguntas, opciones, alternativa manual, verificación del resultado) está en la página de [Instalación](/es/guide/install).

::: info Imágenes precompiladas
La instalación **descarga imágenes precompiladas** de Docker Hub: sin compilar, sin toolchain de Rust/Node. La compilación desde el código fuente sigue disponible para desarrollo (`install.sh --build`, o `./setup.sh --build` desde un clon).
:::
