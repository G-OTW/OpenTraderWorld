# Noticias e investigación

## Noticias {#news}

Un agregador de noticias autoalojado. Crea **dashboards** (p. ej. *Crypto*, *Macro*), añade **fuentes** a cada uno y deja que el scheduler las consulte en segundo plano.

### Fuentes

- **RSS / Atom**: pega la URL de un feed y listo.
- **API (JSON)** para todo lo que no tenga RSS: define el endpoint, el método, las cabeceras y los parámetros de query, y luego mapea rutas JSON a campos del elemento (array de elementos, título, URL, fecha, resumen, id único para deduplicar). Las claves API van en **secretos** por feed, almacenados cifrados y referenciados como <code v-pre>{{secret:NAME}}</code> en cabeceras o parámetros, y nunca se vuelven a mostrar.

La URL, una cabecera o un parámetro de un feed también pueden llevar un marcador <code v-pre>{{vault.item}}</code> que apunta a la [Bóveda](/es/config/settings#vault) compartida. El scheduler lo resuelve en el momento de la consulta, así que una clave se reutiliza entre feeds y módulos sin almacenarse nunca en la configuración del feed.

Cada fuente tiene su propio **intervalo de consulta**; las fuentes duplicadas se detectan para que el mismo feed no se obtenga dos veces entre dashboards. Inicia/detén la consulta por dashboard, o actualiza una fuente bajo demanda.

### Lectura

Filtra elementos por búsqueda, fuente, tipo y rango de fechas; vista compacta o completa; autoactualización opcional cada 60 segundos con un banner "{n} actualizaciones, haz clic para cargar". Un widget de noticias también puede estar en la página de inicio de tu dashboard.

## Buzón {#mailbox}

Tus newsletters, correo de noticias de mercado y correo del broker, leídos desde **tu propio buzón**: nada transita por un tercero.

### Conectar un buzón

Elige tu proveedor (Fastmail, Gmail, iCloud, Zoho, mailbox.org, Posteo, Migadu, Proton Bridge o cualquier otro servidor IMAP) y los ajustes del servidor vienen rellenados; tú aportas una **contraseña de aplicación**, que se almacena en la [Bóveda](/es/config/settings#vault) compartida y en ningún otro sitio.

**Outlook.com / Microsoft 365** ya no aceptan contraseña para IMAP, así que inician sesión con OAuth: se abre una pestaña en Microsoft, apruebas el acceso y vuelve directamente a esta app (código de autorización + PKCE, no se almacena ningún secreto en ninguna parte). Necesita un registro de aplicación propio, único y gratuito: Entra ID → Registros de aplicaciones → nuevo registro, luego Autenticación → *Aplicaciones móviles y de escritorio* con la URI de redirección que te muestra el formulario (`http://localhost:5454/mailbox/oauth` en una instalación local por defecto), flujos de cliente público permitidos, y permisos de API → delegado `IMAP.AccessAsUser.All`. Pega el ID de aplicación (cliente) en el formulario. El sign-in resultante se cifra en la bóveda y se renueva automáticamente en cada obtención.

Esa renovación es lo único que hay que saber de mantenimiento: Microsoft descarta un sign-in tras **90 días sin uso**, así que un buzón que pausaste durante meses pedirá reconectarse. La app avisa tras 60 días inactivo y, si el sign-in se revoca (cambio de contraseña, restablecimiento de MFA, política de administrador), el buzón muestra **Sign-in necesario** con un botón Reconectar en lugar de fallar en silencio.

El acceso es estrictamente de **solo lectura**: la carpeta se abre en solo lectura, y nunca se marca, mueve ni elimina nada en tu servidor. Conecta varios buzones si tienes más de uno.

> Considera una **dirección dedicada** para las newsletters. Así tu correo personal queda fuera de la app por completo, la contraseña de aplicación es revocable con un clic, y el día que un remitente filtre su lista sabrás exactamente cuál fue.

### Qué se conserva

El correo de listas de distribución (todo lo que lleve `List-Unsubscribe`, `List-Id` o `Precedence: bulk`) se conserva automáticamente. Todo lo demás solo se *registra como un remitente a la espera de tu decisión*, y no se almacena contenido hasta que lo archives. Así es como entran los extractos de un broker: un clic en el nuevo remitente, archivado como **Broker**.

Los remitentes se archivan en cuatro categorías (**Noticias**, **Newsletter**, **Broker**, **Otros**), cambiables en cualquier momento, y la pantalla de lectura tiene un interruptor de un clic por categoría y un filtro por buzón cuando tienes varios.

### Lectura

Los mensajes se sanean al llegar (se eliminan scripts, estilos, formularios y marcos) y se muestran en un marco aislado. **Las imágenes remotas permanecen bloqueadas** hasta que las pides, así que el píxel de seguimiento de una newsletter nunca se dispara y el remitente no puede saber que la abriste. Los adjuntos (extractos del broker, PDF) se pueden descargar desde el mensaje.

Por mensaje: destacar, marcar como no leído, archivar, **Recuérdamelo** (esta noche / mañana / este fin de semana, directo a [RemindMe](/es/modules/productivity#remindme)) y **Cancelar suscripción**, enviado por ti cuando el remitente admite un clic, abierto en una pestaña en caso contrario.

### La tienda

La pestaña **Tienda** es tu propia lista de newsletters: una tarjeta por publicación con un nombre, un enlace, una breve descripción y un tema (mentalidad, finanzas, trading, geopolítica, economía, otros), agrupadas por dominio y abribles con un clic. Es independiente, útil incluso sin ningún buzón conectado.

## Calendario Económico {#economics}

Próximos eventos macro (decisiones de bancos centrales, publicaciones del IPC, datos de empleo) en una vista de calendario, para que sepas lo que viene antes de tu sesión. Un clic añade un recordatorio para un evento.

## FinanceDatabase {#findb}

Un catálogo con búsqueda de **más de 300.000 instrumentos**: acciones, ETF, fondos, índices, divisas y criptomonedas.

En el primer uso **instalas el catálogo** (una descarga única de ~15 MB, importada en segundo plano). Después vive en local y **las búsquedas nunca tocan la red**. Busca por símbolo o nombre, filtra por tipo de activo y atributos, y marca instrumentos con estrella en **favoritos**, organizados en carpetas con notas (p. ej. una carpeta *Watchlist*).

El catálogo tiene su propio ciclo de versiones, separado de la app: la cabecera muestra qué **instantánea** está instalada y un botón **Buscar actualizaciones** pregunta al editor si existe una más nueva. Actualizar reimporta el catálogo en el sitio; tus favoritos sobreviven y se revinculan a las nuevas filas.

El catálogo se construye a partir del proyecto de código abierto [FinanceDatabase](https://github.com/JerBouma/FinanceDatabase) de Jeroen Bouma, un dataset de instrumentos financieros mantenido por la comunidad.

## Recursos {#resources}

Una biblioteca de marcadores para libros, artículos, vídeos y herramientas de trading: nombre, enlace opcional, descripción, organizados en categorías. Simple a propósito.

Tres vistas: **tarjetas**, **lista** y una **galería** con una miniatura por marcador. Una miniatura se sube, se pega como URL o se obtiene con un clic de la vista previa social del propio enlace; un marcador sin una recibe un mosaico con iniciales en lugar de un hueco en la cuadrícula.

## Docs de la comunidad {#community-docs}

Guías escritas por la comunidad, sincronizadas desde la [biblioteca en opentraderworld.com](https://opentraderworld.com/docs) y **legibles sin conexión** dentro de la app. Explora por categoría, busca y marca favoritos con estrella.

Los docs se muestran como **tarjetas o como lista**, a tu elección, y una tarjeta de categoría previsualiza los docs que contiene para que sepas qué hay dentro antes de abrirla.

Puedes contribuir: escribe un documento en el [Editor](/es/modules/productivity#editor) y usa **Enviar para publicación**. Va a una cola de revisión y aparece en la biblioteca de todos una vez aprobado.
