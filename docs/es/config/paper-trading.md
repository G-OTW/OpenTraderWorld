# Cuenta de simulación

Una **cuenta de paper trading** es un broker que gestiona el propio OpenTraderWorld. Las órdenes que recibe se ejecutan al precio en directo del gráfico del que salen, contra su propio efectivo simulado, con las comisiones, el slippage y el apalancamiento que usted fije. Sin clave, sin mercado, sin dinero real.

Se opera desde el gráfico exactamente como un broker real: el mismo ticket, la misma barra rápida, las mismas líneas de órdenes y posiciones, pestañas del panel y registro de auditoría. Todo eso se describe en [Operar desde el gráfico](/es/config/trading); esta página cubre lo propio de la simulación.

## Crear una

**Ajustes → Brokers → Añadir cuenta**, broker **Paper trading**. No hay credenciales. Los ajustes (rotulados en inglés por la aplicación):

| Ajuste | Por defecto | Significado |
|---|---|---|
| **Account currency** | `USD` | La divisa del efectivo y de los resultados, de 3 a 5 letras |
| **Starting balance** | `100000` | Se anota como primer depósito |
| **Commission per order** | 0 | Un importe fijo por ejecución |
| **Commission per unit** | 0 | Por acción o contrato |
| **Commission (% of value)** | 0 | `0.1` significa 0,1 % del valor negociado |
| **Minimum commission** | 0 | Una ejecución nunca cuesta menos |
| **Slippage (basis points)** | 0 | `5` significa 0,05 %, en su contra |
| **Leverage** | 1 | 1 es una cuenta al contado |
| **Short selling** | yes | `no` rechaza una venta sin nada que vender |
| **Fill on** | `quote` | `quote` opera al bid y al ask, `last` al último precio |

Los números deben ser cero o más, y el apalancamiento al menos 1. Los ajustes se releen en cada ejecución: un cambio se aplica desde la siguiente.

Una cuenta paper nueva tiene activados **Todos los módulos** y **Trading**. El trading en una cuenta paper no pide contraseña y nunca está detrás del [bloqueo de trading](/es/config/trading#trading-lock).

**Probar conexión** responde con el equity, el efectivo, las posiciones abiertas y las órdenes activas de la cuenta.

## Efectivo

El panel **Cuenta paper**, bajo la cuenta en Ajustes → Brokers, tiene un campo **Importe** y tres botones:

- **Depositar** suma el importe al efectivo.
- **Retirar** lo saca, hasta el efectivo disponible (no el equity).
- **Reiniciar la cuenta** borra cada orden, ejecución y movimiento de efectivo, y vuelve a empezar con un único depósito: el importe escrito, o el saldo inicial si el campo está vacío. Los ajustes se conservan, y los trades ya importados al journal se quedan.

El panel muestra también el total depositado, y las últimas 50 órdenes que dejaron de estar activas con su estado (ejecutada, cancelada, rechazada, caducada) y el motivo. **Esa lista es donde se explica un rechazo.**

## Cómo se ejecutan las órdenes

### A qué precio

Una orden se ejecuta con el **flujo en directo del conector de datos del gráfico**: el conector del que vino la orden, o el conector del proveedor concedido a Gráficos. El motor corre en el servidor, así que **las órdenes se siguen ejecutando con el gráfico cerrado**, y se detienen mientras la aplicación está apagada.

- Un instrumento sin flujo en directo se rechaza al enviar la orden. Un índice también: no hay mercado que operar.
- El motor sigue las barras en directo más finas del proveedor (1 minuto si está disponible) y sus cotizaciones cuando el flujo las publica. Solo cuentan los precios impresos después de que la orden empiece a estar activa.
- Con **Fill on: quote**, las compras se ejecutan al ask y las ventas al bid, mientras la cotización tenga menos de 15 segundos. Si no, y con **last**, ambas al último precio.
- Una barra de más de 2 minutos no se usa. Una orden a mercado enviada con el mercado cerrado espera el primer precio fresco en lugar de rechazarse.
- Si el flujo se detiene, se reintenta cada 10 segundos y las órdenes esperan. Si se borra el conector de datos, sus órdenes ya no pueden ejecutarse.

### Por tipo de orden

| Tipo | Se ejecuta cuando | A |
|---|---|---|
| **Market** | el siguiente precio | ese precio, más slippage |
| **Limit** | el mercado alcanza el límite | el límite, o mejor si el mercado ya lo ha superado. Sin slippage |
| **Stop** | el mercado alcanza el stop | el stop si se toca, el mercado en un gap, más slippage |
| **Stop limit** | se alcanza el stop, luego el límite | como un limit, quizá al mismo precio |
| **Trailing stop** | el mercado retrocede la distancia de seguimiento desde su mejor precio desde que empezó el stop (el más alto tras una venta, el más bajo tras una compra) | como un stop |
| **MIT** | el mercado llega al disparador por el lado del limit | el mercado, más slippage |
| **LIT** | el disparador, luego el límite | como un limit |

- **Sin ejecuciones parciales**: una orden se ejecuta entera, sin comprobar volumen ni liquidez.
- Un **trailing stop** con precio de inicio espera a que el mercado lo alcance (subir hasta él tras una venta, bajar hasta él tras una compra) y luego sigue desde ese precio. Su mejor precio se conserva tras un reinicio.
- **Reduce only**: colocada sola, se rechaza salvo que la cuenta mantenga una posición del otro lado. Al ejecutarse se limita a lo que se mantiene entonces, y se cancela cuando no queda nada que reducir.
- **Post only**: un limit rechazado al colocarse cuando se ejecutaría de inmediato contra el bid o ask actual (el último precio si no hay cotización).
- Sin iceberg, elección de disparador, fuera de horario regular, close-position ni algo: un precio en directo sin profundidad, sesión ni mark no tiene sobre qué jugarlos, por eso el ticket no los ofrece.
- El precio de ejecución se redondea al tick del instrumento cuando se conoce.
- **Comisión** por ejecución = por orden + por unidad × cantidad + % × valor negociado, elevada al mínimo. Se cobra en la divisa del instrumento.

### Vigencia

- **Day** (por defecto) caduca a la siguiente **medianoche UTC**, no al cierre de la bolsa.
- **GTC** no caduca nunca. **GTD** caduca en la fecha y hora indicadas.
- **IOC** y **FOK** se cancelan si el primer precio que encuentran no las ejecuta. Con el mercado cerrado, esperan ese primer precio. Paper ejecuta entero, así que las dos acaban igual.

La caducidad se comprueba aproximadamente cada 10 segundos; la orden aparece entonces como caducada.

### Brackets y OCO

- Un **take profit** o **stop loss** espera a que se ejecute su entrada y luego pasa a estar activo. Si la entrada se cancela, se rechaza o caduca, sus salidas se cancelan.
- En un **OCO**, una ejecución cancela la otra orden del grupo, y sus salidas.
- Los precios, la cantidad y la vigencia de una orden activa se pueden cambiar. **Cerrar posición** envía una orden a mercado GTC y luego cancela las salidas de la posición.

## Fondos y apalancamiento

- Una posición inmoviliza su valor ÷ apalancamiento. Los futuros inmovilizan su margen por contrato (ver abajo) e ignoran el apalancamiento.
- Una orden que abre o amplía una posición se comprueba al enviarla y de nuevo al precio de ejecución, comisión incluida. Se rechaza si la cuenta inmovilizaría más que su equity:

  > not enough funds: the positions would tie up X USD and the account's equity is Y USD

- Una orden que reduce una posición siempre se acepta.
- Con **Short selling** en `no`, una venta que quedaría en corto se rechaza.
- **No hay llamadas de margen ni liquidación forzosa**: el equity puede caer por debajo del margen usado.

## Futuros

Un futuro necesita los datos de su contrato, en los **Ajustes del instrumento** del gráfico:

- **Tamaño del tick, valor del punto y divisa.** Si están vacíos, se rellenan con las especificaciones de contratos de código abierto de [QuantConnect Lean](https://github.com/QuantConnect/Lean), integradas en la aplicación y actualizadas cada semana. El diálogo nombra los campos rellenados; guarde para conservarlos, o escriba los suyos. Sus valores guardados siempre prevalecen.
- **Margen por contrato (paper)**, en la divisa del instrumento: use el margen inicial de su broker. O marque **Contar el contrato a su valor completo**: cantidad × precio × valor del punto.

Una orden sobre un futuro sin margen se rechaza, y el gráfico ofrece **Fijar margen** (abre los ajustes del instrumento) o **Usar valor completo**. Cualquiera de las dos reenvía la orden.

Un ticker se reduce a la raíz de su contrato (`MNQZ6`, `ESZ25`, `@ES`, `ES=F`, `/ES`). Una raíz listada en varias bolsas sin bolsa indicada es un error que nombra las candidatas: **el mercado nunca se adivina**. Un futuro u opción sin valor del punto se rechaza, igual que cualquier instrumento sin divisa, salvo un par de forex, cuya divisa se lee de su nombre (`EURUSD` está en USD).

## Cifras de la cuenta

La pestaña **Cuenta** del panel de órdenes y el panel de Ajustes muestran:

| Cifra | Cálculo |
|---|---|
| **Efectivo** | depósitos − retiros + realizado − comisiones |
| **Resultado abierto** | Σ (precio − precio medio) × cantidad × valor del punto |
| **Equity** | efectivo + resultado abierto |
| **Realizado** | desde el último reinicio, antes de comisiones, a coste medio |
| **Comisiones** | total de comisiones |
| **Margen inicial** | margen por contrato de los futuros + valor del resto de posiciones ÷ apalancamiento |
| **Disponible** | equity − margen inicial, nunca bajo cero |
| **Valor de las posiciones** | exposición bruta, larga y corta |

La pestaña se actualiza cada 5 segundos mientras está abierta.

### Otras divisas

La cuenta tiene una sola divisa. Un instrumento en otra se convierte con el **tipo de cambio del día del journal**: resultados realizados y comisiones al tipo de su ejecución, resultados abiertos y margen al de hoy.

- Una orden se rechaza si no existe tipo: `no EUR to USD rate: add one in the journal's FX settings, or trade in USD`.
- Una posición cuyo tipo desaparece después figura como excluida de los totales.

## Al journal

Las ejecuciones paper se importan como las de cualquier broker: **Journal → Importar → Desde un broker**, elija la cuenta paper, el libro y el periodo, previsualice, importe. Vea [Importar operaciones al journal](/es/config/brokers#import-trades-into-the-journal). La **sincronización automática** también funciona.

## Conviene saber

- Los importes se guardan como decimales exactos, como los contabiliza un mercado: 0,1 comprado tres veces y 0,3 vendido no deja nada abierto, y un precio debe caer exactamente en el tick del instrumento.
- El efectivo, las órdenes y las ejecuciones se guardan en la base de datos y sobreviven a los reinicios. Los precios en directo no: tras un reinicio, las posiciones se valoran a su precio medio hasta la siguiente cotización.
- Borrar la cuenta borra su efectivo, sus órdenes y sus ejecuciones.
- Cada llamada de orden está en el [registro de auditoría](/es/config/trading#audit-log).
- No se modela: posición en la cola, liquidez, ejecuciones parciales, intereses, costes de préstamo, funding, dividendos.
- Las órdenes y movimientos de efectivo paper no están expuestos a los [agentes de IA (MCP)](/es/config/ai-agents), y las cuentas paper están desactivadas en [modo demo](/es/guide/demo).
