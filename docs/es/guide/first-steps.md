# Primeros pasos

Has [instalado](/es/guide/install) OpenTraderWorld y tienes tus credenciales de administrador. Así lo haces tuyo.

## Iniciar sesión

Abre la app e inicia sesión en `/login`. Si tu contraseña fue **generada por el instalador**, se te pide **elegir una nueva en el primer sign-in**, ya que la contraseña generada solo sirve una vez.

Puedes cambiar tu usuario o contraseña en cualquier momento en **Ajustes → Cuenta**. Cambiar la contraseña cierra todas tus sesiones. ¿Bloqueado? No hay correo de restablecimiento: se recupera desde la shell del host, consulta [Olvidé mi contraseña](/es/guide/troubleshooting#forgot-password).

Después, en **Ajustes → Seguridad**: activa la **autenticación en dos factores** y revisa qué navegadores tienen la sesión iniciada. Hazlo antes de dejar que algo ajeno a esta máquina alcance la app. Consulta [Seguridad de la cuenta](/es/config/security).

## Define tus valores predeterminados

Ve a **Ajustes → Valores predeterminados** y elige:

- **Idioma**: se aplica a toda la app de inmediato (inglés, francés, alemán, español, italiano, portugués, chino).
- **Divisa predeterminada** y **zona horaria**: se usan como valores iniciales en todos los módulos.

## Instala tus módulos

Abre **Ajustes → Módulos**. Todos los módulos vienen con la app; instalar uno solo lo hace disponible en el selector de módulos y en el dashboard, y no se descarga nada.

- **Instala** los módulos que quieras. Empieza con pocos, puedes añadir más en cualquier momento.
- Algunos módulos dependen de otros: **Visualización de datos históricos**, **Backtest** y **Herramientas Cuant** necesitan **Datos históricos** (trabajan sobre sus datasets descargados).
- **Desvincular** oculta un módulo y lo hace inaccesible; sus datos se conservan salvo que marques también *eliminar datos*. Puedes reinstalarlo en cualquier momento.

¿No sabes por dónde empezar? Consulta el [resumen de módulos](/es/modules/) para ver qué hace cada uno.

## Muévete por la app

- **Selector de módulos** (arriba a la izquierda): salta entre los módulos instalados. Cada módulo tiene todo su área de trabajo: su propia barra lateral, páginas y contenido.
- **Dashboard** (inicio): un tablero de mosaicos y widgets de tus módulos, con tantas páginas como quieras. Consulta [Dashboard y navegación](/es/modules/dashboard).
- **Búsqueda** (barra superior, <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, o <kbd>/</kbd>): encuentra módulos y secciones de ajustes; el interruptor de capas la amplía a tu propio contenido.
- **Campana** (barra superior): la bandeja de notificaciones, donde llegan los recordatorios y las alertas de webhooks.
- **Ajustes**: cuenta, valores predeterminados, apariencia, red, módulos, datos, copia de seguridad, actualizaciones, registros, connectors, bóveda y más. Consulta la [referencia de ajustes](/es/config/settings).

## Próximos pasos recomendados

1. **Adquiere pronto el hábito de las copias de seguridad**: consulta [Copia y restauración](/es/guide/backup-restore).
2. Si otros dispositivos deben acceder a la app, lee [Red y acceso remoto](/es/config/network) antes de cambiar nada, y activa primero la [autenticación en dos factores](/es/config/security#totp).
3. ¿Usas proveedores externos de datos de mercado? Crea un **[connector de datos](/es/config/connectors)** por cuenta en **Ajustes → Connectors de datos** según los necesites. La app funciona bien sin ninguno, y varios proveedores no requieren clave.
