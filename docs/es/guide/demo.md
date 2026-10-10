# Modo demo

Hay un sandbox público en **[demo.opentraderworld.com](https://demo.opentraderworld.com)**: la app real, con datos de ejemplo, compartida por todos y restablecida cada **15 minutos**. Nada que instalar, nada para registrarse; entras con la sesión iniciada como `demo`.

Esta página explica qué es ese modo, para que sepas qué estás viendo, y para que puedas ejecutar uno tú mismo si quieres enseñar la app a alguien.

## Qué es

El modo demo es una postura que adopta el backend cuando arranca con `OTW_DEMO=1`. **Nunca se activa de forma implícita**: una instalación normal no se ve afectada por nada de lo que sigue.

- **La base de datos se restablece cada cuarto de hora.** La semilla se restaura desde una plantilla, así que todo lo que cambies desaparece en el minuto :00, :15, :30 o :45. El banner de la app cuenta atrás hasta el siguiente.
- **Entras automáticamente** con la cuenta `demo`. No hay contraseña que adivinar ni cuenta que crear.
- **Todos comparten una base de datos.** Las operaciones, notas y conversaciones de otros visitantes son visibles, y las tuyas son visibles para ellos. No escribas nada que no publicarías.

## Qué está bloqueado

La puerta es de **denegación por defecto**: una petición debe coincidir con una lista de permitidos explícita o se rechaza con `demo_disabled`. Una ruta en la que nadie pensó queda cerrada, no abierta, la misma regla que sigue el [catálogo MCP](/es/config/ai-agents).

A grandes rasgos:

| Bloqueado | Solo lectura | Completo |
|---|---|---|
| Configuración inicial, cierre de sesión, cuenta y contraseña, red, copia de seguridad, actualización, borrado de datos, instalar/desvincular módulos, **la bóveda**, **el Automator**, webhooks entrantes, el propio endpoint MCP, canales de notificación, instalación de FinanceDatabase, descargas de proveedores | Connectors de datos, feeds, archivos, carteras de gestores, ajustes y tokens MCP, tasa de API, datasets almacenados, proveedores del agent, memorias y skills | Journal, backtest, cuant, portfolios, calendario, tareas, objetivos, editor y bases de datos, prompts, recursos, suscripciones, taxcalc, control de tiempo, dashboard, búsqueda, chat del agent |

Así que puedes registrar una operación, ejecutar un backtest y hablar con el asistente; no puedes cambiar el modo de red, generar un token, volcar la base de datos ni hacer que la máquina consulte a un proveedor de pago en tu nombre.

Dos de ellos merecen una palabra, porque el módulo es visible pero no hace nada:

- **La bóveda** está cerrada del todo. Es el único almacén cuyo propósito entero es guardar credenciales, y esta base de datos es compartida y pública.
- **El Automator** está cerrado por la regla de denegación por defecto y no por una línea propia: un workflow llega a todo lo que su token concede y puede llamar a cualquier URL, justo lo que un sandbox público no debe ofrecer. Al abrir el módulo se muestra la interfaz; cada petición que hay detrás responde `demo_disabled`.

Los mensajes de chat también están limitados a 2000 caracteres.

## Cuotas por visitante

Los endpoints costosos cuestan dinero real, así que cada uno lleva **dos** presupuestos: una porción por visitante y un techo global encima. Solo por IP no limitaría el gasto; solo global permitiría que un visitante con un script bloqueara a todos los demás.

| | Por visitante | En toda la demo | Ventana |
|---|---|---|---|
| **Ejecuciones del agent** | 3 | 8 | 10 minutos |
| **Ejecuciones del agent** | 10 | 40 | 24 horas |
| **Backtests y barridos** | 10 | 30 | 10 minutos |

El asistente se ejecuta con una **clave compartida fijada a un modelo gratuito**, resuelto al arrancar; la memoria a largo plazo y los servidores MCP externos están desactivados.

## Ejecutar la tuya

```bash
OTW_DEMO=1        # in the core service's environment
otw-core --seed-demo   # once, against a scratch database
```

La semilla es pública: viene en el repositorio, no contiene secretos y se interrumpe si ya existe un usuario `demo`. El restablecimiento es una restauración con `CREATE DATABASE … TEMPLATE` dirigida desde el host, no por la app.

::: warning No apuntes el modo demo a tus datos
La semilla escribe en lo que nombre `DATABASE_URL`, y el restablecimiento restaura por encima. Usa una base de datos de pruebas.
:::
