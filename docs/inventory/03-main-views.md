# Inventario: vistas principales

Origen: `~/drplayer/DrPlayer/Sources/`. Mini-player excluido.

Transversal:
- **Atajos**: solo Escape (`NowPlayingFullView.swift:47`, `.onExitCommand` en `PlayerView.swift:453`).
- **Sin drag & drop, sin doble clic**: todo se activa con un clic.
- **Color DR** (duplicado en 6 vistas): ≥14 verde, 10–13 amarillo, 7–9 naranja, <7 rojo.
- **Barra DR**: cápsula rellena a `dr/20` (máx. 1); 60×6 en AlbumDetail, 80×6 en TrackDetail.
- **Color de formato**: DSF/DFF/DSD verde, FLAC/WAV/AIFF azul, MQA morado, MP3/AAC naranja, resto secundario.
- **Hover**: solo cursor mano → `set_cursor_from_name("pointer")`.
- **Bitperfect**: delegado a `SignalPathIndicator` (ver `04-secondary-views.md`).

Para el port: centralizar colores DR/formato y la lógica de versiones/ediciones (está duplicada).

---

## PlayerView (ventana raíz)

**Layout** (`PlayerView.swift:77-463`): NowPlayingBar · divisor · [contenido | Letras 320 px | Cola 300 px] · tira osciloscopio opcional · línea de error. Encima: NowPlayingFullView y sheet de primera ejecución. Contenido = 4 capas vivas (vacío, navegación, AlbumDetail por álbum, búsqueda) para conservar scroll. Sin sidebar: fila segmentada de botones.

- [ ] **Modos**: Home, Albums, Artists, Labels, Tracks, Composers, Producers, Smart, Stats; activo en negrita sobre acento; Composers oculto si no hay compositores. — `:18-28`, `:125-142`
- [ ] **Chip de filtro** (solo Albums): icono por tipo (genre/label/country/format), nombre, × para limpiar. — `:145-160`, `:45-52`
- [ ] **Filtros**: genre ci en `album.genres`; label en álbum o cualquier pista; country en cualquier pista; format `contains`. — `:54-69`
- [ ] **Estadística**: `"N albums · M tracks"`. — `:164-166`, `:465-469`
- [ ] **Estado vacío**: botón "Load library"; si `libraryIsEmpty`, "MPD's library is empty. Is the music disk mounted on the server?". — `:101-114`
- [ ] **Abrir álbum**: `vm.loadRatings` antes; guarda `lastAlbumId` para restaurar scroll. — `:177-183`
- [ ] **Cableado por modo**: Labels → filtro + Albums + abre primer álbum; Artists → limpia filtro + Albums + primer álbum del artista; Producers con `navigateToProducer`. — `:175-300`
- [ ] **Callbacks AlbumDetail**: play/enqueue/radio, play pista, enqueue pista, versiones, preferida (+ recarga stickers), rating, back, filtros (cierran detalle), búsqueda, productor, scan DR, `updateDB` (+ relee álbum y ratings). — `:309-388`
- [ ] **Búsqueda**: abrir álbum (+ ratings, cierra búsqueda), play, enqueue. — `:396-414`
- [ ] **Osciloscopio 120 px** si `showVisualizer && isPlaying`. — `:433-437`
- [ ] **Error** en rojo. — `:439-444`
- [ ] **Ciclo de vida**: `vm.start()` / `vm.stop()`. — `:446-447`
- [ ] **Full player**: overlay desde abajo, Escape cierra. — `:450-454`
- [ ] **Setup inicial**: no descartable mientras `!hasCompletedSetup`; al terminar `vm.reconnect()`. — `:456-462`

GTK: `gtk::Stack` (mantiene scroll), `gtk::Revealer` SlideUp o `adw::NavigationView`, `ShortcutController` para Escape, `adw::Dialog` con `can-close=false`.

## NowPlayingBar

**Layout** (`PlayerView.swift:483-674`): info (portada, texto, audio) · waveform/progreso · transporte · utilidades.

- [ ] **Portada** 48×48 r6; clic → full player; placeholder si stop y `"---"`; carga async `findCover` del item en `currentPos`. — `:488-492`, `:678-688`, `:739-775`
- [ ] **Texto**: título; artista — álbum (álbum clicable → abre álbum actual + ratings). — `:495-522`, `:81-90`
- [ ] **Info audio**: `formatAudio` → `"DSD64 DoP stereo"` / `"44.1kHz/24bit stereo"` (kHz enteros sin decimal; mono/stereo/`Nch`); `"N kbps"` (oculto si vacío/0); `"DRn"` coloreado. — `:529-543`, `:707-734`
- [ ] **Progreso**: waveform 32 px (jugado 0.8, resto 0.2) o barra 4 px; tiempos `m:ss`. **Sin seek.** — `:550-581`, `:692-696`
- [ ] **Transporte**: shuffle (acento si on), prev, play/pause, stop, next, repeat (icono/tooltip por modo), `SignalPathIndicator` compacto. — `:586-616`
- [ ] **Utilidades**: Search, Lyrics, Queue ("Play queue (N tracks)"), Oscilloscope, Radio (verde si activa; stop/startFromCurrent), `BackgroundTasksIndicator`. ~~Mini Player~~. — `:621-669`

## HomeView

**Layout** (`HomeView.swift:13-45`): scroll vertical de estanterías horizontales de AlbumCells 140 px.

- [ ] Spinner hasta construir secciones. — `:16-23`
- [ ] **"Recently Played"**: historial, más reciente primero, dedupe por carpeta, máx. 20. — `:25-27`, `:54-63`
- [ ] **Estanterías por género**: top 8 géneros por duración escuchada; ≤12 álbumes por plays; solo si ≥2. — `:29-31`, `:84-99`
- [ ] **"Not Played Yet"**: 12 álbumes aleatorios nunca reproducidos. — `:33-35`, `:102`
- [ ] Indicador de reproducción si `album.title == vm.currentAlbum`; clic abre álbum. — `:122-124`
- [ ] Se construye al aparecer; sin refresco. — `:42-44`

## AlbumGridView / AlbumCell

- [ ] **Grid adaptativo** 160–200 px, spacing 16. — `AlbumGridView.swift:13-38`
- [ ] **Restaurar scroll** a `scrollToAlbumId` centrado (animado al cambiar, sin animar al aparecer). — `:25-36`
- [ ] **Celda**: portada cuadrada r8 (placeholder), badge de reproducción abajo-derecha, título 2 líneas, artista 1 línea; `CoverCache` sync y luego `coverImageAsync`. — `:41-97`
- Sin menú contextual ni orden.

GTK: `gtk::GridView` + `SignalListItemFactory`; `scroll_to` (GTK ≥4.12).

## AlbumDetailView

**Layout**: sub-pantallas TrackDetail / EditionComparison / scroll principal: "‹ Library" · cabecera 3 columnas [metadatos + acciones | AlbumInfoView | carrusel] · géneros · galería plegable · otras ediciones · pistas. — `AlbumDetailView.swift:58-309`

- [ ] **Cabecera**: fecha (clic → busca), título grande 3 líneas, artista (clic → busca). — `:175-181`, `:756-765`
- [ ] **Acciones**: Play, Add to queue, Radio ("Generate playlist based on this album"), Compare editions. — `:184-212`
- [ ] **Menú "More"**: — `:214-281`
  - [ ] "Open in drtagger" (bundle `us.priet.drtagger-mac`; deshabilitado si no está)
  - [ ] "Open in Dr. Doctor"
  - [ ] "Refresh tags" (limpia releases, `updateDB(folder)`, recarga info)
  - [ ] "Retry cover art" (limpia cachés, recarga slot 0)
  - [ ] "Reveal in Finder"
- [ ] **Metadatos** (orden): Duration, Tracks, Format (clic → filtro), Dynamic Range (`avgDR` en vivo durante el scan + barra), Label (filtro), Catalog, Country (filtro), Producer/Engineer/Mastering (créditos MB clicables → Producers; fallback tags `PRODUCER`/`ENGINEER`), Mixer (tag, si no hay ingenieros MB), Images. — `:551-722`, `:1241-1265`
- [ ] **Precedencia Label/Catalog/Country**: tag del fichero (MPD o ffprobe) → MB solo si `mbMatchesEdition` (label fuzzy + catno normalizado; limpia (P)(C)℗©) → Discogs. — `:999-1076`
- [ ] **Badge prensaje certificado**: label · país; catno · año; formatos; detalles Mastered by / Cut by / Pressed at / Recorded at / Mixed at / Matrix. — `:295-298`, `:476-547`
- [ ] **Carrusel**: sin imágenes → placeholder + "Retry"; imagen ≤240 px; botones "Identify pressing" y "Scan OBI" (ArtworkServer); clic → zoom ≤600 px; swipe ≥30 px; chevrons + `"i/N"`; carga perezosa. — `:313-472`, `:141-156`
- [ ] **Géneros**: chips en flow layout, mayúsculas; clic → filtro. — `:726-752`, `:1339-1378`
- [ ] **Galería**: si >1 imagen, "Artwork (N images)" plegable con `ArtworkThumbnail`. — `:769-801`
- [ ] **Otras ediciones**: mismo artista (ci, sin diacríticos) y título sin `(...)`/`[...]`; orden por formato; fila: miniatura 40, título, badge formato, fecha, DR, "N tracks"; clic carga edición; menú contextual y "…": Play, Add to queue, Analyze in Dr. Doctor, Compare with current. — `:805-995`, `:1054-1060`
- [ ] **Pistas**: `TrackRow` con divisores; clic → TrackDetail; rating actualiza localmente al instante. — `:1292-1334`
- [ ] **Al abrir**: `loadReleaseInfo()` — `:1147-1236`:
  - MB (primer acierto): `fetchRelease(mbid)` → `searchByCatalog(artist, catno, label)` → `searchByCatalog(artist, catno)` → `searchRelease` con hints (`detectHints` `:1130-1145`) → con label → genérico → título crudo.
  - Discogs: catno → catno de MB → barcode de MB → barcode del fichero → búsqueda.
  - Si Discogs da otro catno, reintenta MB con él.
  - Limpieza de título `:1079-1107`, catno desde título `:1110-1127`.
- [ ] **Scan DR automático** al abrir. — `:160-165`

Linux: drtagger/DrDoctor/OBIScanner probablemente no existen → acciones condicionales vía `gio::DesktopAppInfo`. "Reveal" → `gtk::FileLauncher::open_containing_folder`; enlaces → `gtk::UriLauncher`; `adw::Carousel`; `gtk::PopoverMenu` con `GestureClick` secundario.

## AlbumInfoView (columna central)

- [ ] **Cargando**: "Loading...". — `AlbumInfoView.swift:28-34`
- [ ] **Badge resumen MB** (si label casa): label · país · fecha / "Cat. X" · status · formato Discogs; enlaces MB/Discogs/Wikipedia. — `:39-43`, `:109-169`
- [ ] **Badge Discogs** (fallback). — `:171-205`
- [ ] **"About the album"**: Wikipedia, 6 líneas + "Read more on Wikipedia". — `:46-48`, `:220-244`
- [ ] **"Sobre <artista>"** (en español en el original) si su URL difiere de la del álbum. — `:51-54`
- [ ] **"DETAILS"** + secciones plegables (cerradas, animadas, "Source: X"): Release (MusicBrainz), Physical edition (Discogs; "Min. price $X", notas 4 líneas, "View on Discogs"), Credits (por rol), Members (área · "since", miembros). — `:58-93`, `:248-370`
- [ ] **Vacío**: "No additional information". — `:95-99`
- [ ] **Carga**: tags vía ffprobe (LABEL/PUBLISHER, CATALOGNUMBER, BARCODE/UPC/EAN); cadena MB en paralelo con `searchArtist`; Wikipedia álbum: slug → `"<album> <artist> album"` → `"<album> (<artist> album)"` → `album` (solo si el extracto menciona al artista); Wikipedia artista: slug → `searchArtist`; Discogs + reintento MB. — `:399-601`
- Duplica las búsquedas de AlbumDetailView → **en el port, un único servicio de "release info" compartido**.

## TrackDetailView

- [ ] **Cabecera**: "‹ Back", título, artista, álbum clicable si existe; Play, "Add to queue", "Dr. Doctor". — `TrackDetailView.swift:93-161`
- [ ] **Cargando** mientras `TrackProbe.probe`. — `:39-42`, `:83-88`
- [ ] **Audio**: Codec, Sample Rate, Bit Depth (oculto si 0), Sample Format, Channels `"n (layout)"`, Duration `"m:ss (s.sss s)"`, Bitrate. — `:173-200`
- [ ] **File**: ruta MPD, formato, tamaño (base 1000). — `:202-214`, `:271-278`
- [ ] **Tags** ordenados, ocultando prefijos musicbrainz, acoustid, script, replaygain, encoder, barcode, asin, isrc, catalognumber, media, totaldiscs, totaltracks, tracktotal, disctotal, releasestatus, releasetype, releasecountry. — `:216-235`
- [ ] Valores seleccionables. — `:267`
- [ ] **DR** + barra 80 px. — `:237-253`
- [ ] **Otras versiones (N)**: título normalizado, artista laxo; orden formato, álbum; fila: play, álbum, badge formato, DR + barra, duración, label/país/fecha/"(orig. Y)", carpeta 9 pt truncada por delante; clic → álbum. — `:330-466`

## TrackListView (modo Tracks)

- [ ] **Filtro** ci en título, artista, álbum, compositor, sello; "Search tracks..." con ×. — `TrackListView.swift:27-36`, `:80-92`
- [ ] **Orden**: Title, Artist, Album, Label, Composer, Duration; asc/desc. — `:15-22`, `:38-55`, `:94-109`
- [ ] **Recuento** "N tracks". — `:111-113`
- [ ] **Columnas**: play 28, Title flex, Artist 140, Album 140, Label 100, Composer 100, DR 40 (no ordenable), Dur. 50; clic en cabecera ordena/invierte. — `:121-173`
- [ ] **Fila**: play → `enqueueAndPlay`, campos, DR, duración; clic → TrackDetail. — `:175-239`
- Bug original: "Add to queue" desde aquí reproduce (`:66`) → no replicar.

GTK: `gtk::ColumnView` + `SortListModel`/`FilterListModel`.

## TrackRow

- [ ] Fila: icono (play/altavoz), nº (o índice+1), título (+ artista si difiere del del álbum), versiones, encolar, DR, 5 estrellas, duración. Clic → TrackDetail. — `TrackRow.swift:22-78`
- [ ] **Pista actual**: fondo acento 0.08, título negrita. — `:57`, `:71`
- [ ] **Badge versiones** → popover "Compare versions": incluye la actual, `rankVersions`; fila: play/altavoz, álbum (negrita si mejor, clic → álbum), **BEST** / **PREFERRED** / **CURRENT**, formato, fecha, DR (o "DR?"), **"Npts"**, botón "Set as preferred version"; fondo verde si mejor, rojo si warning. — `:85-246`
- [ ] **Encolar** ("Add to queue"). — `:336-346`
- [ ] **Badge DR** → `DRComparisonPopover` ("Compare DR with other editions on Loudness War DB"). — `:349-374`
- [ ] **Rating**: 5 estrellas; clic en la actual → 0. — `:385-396`

GTK: gestos hijos que reclaman el evento para no activar la fila.

## QueueView (panel 300 px)

- [ ] Cabecera "Queue", "N tracks", papelera → `clearQueue` ("Clear queue"). — `QueueView.swift:9-28`
- [ ] Fila: actual con altavoz + negrita, si no posición; título, artista; × → `removeFromQueue`; clic → `playTrack(pos)`. — `:34-74`
- Sin reordenar, sin autoscroll, sin estado vacío.

## SearchView

- [ ] "‹", campo "Search artists, albums, tracks..." con foco a los 0.2 s, ×. — `SearchView.swift:25-47`, `:108-116`
- [ ] Busca en cada cambio sin debounce; <2 caracteres → vacío. — `:117-129`
- [ ] Estados: "Type to search", spinner, `No results for "<q>"`. — `:58-77`
- [ ] **Matching local ci**: álbumes por título/artista/género; artistas de álbumes + pistas con recuentos (orden por pistas desc); pistas por título/artista. — `:124-181`
- [ ] **Límites**: 10 artistas, 20 álbumes, 30 pistas (cabeceras con total). — `:84-100`
- [ ] Fila artista → reemplaza la consulta por el nombre. — `:199-221`
- [ ] Fila álbum: miniatura 40, título, artista, formato, DR → abre. — `:223-254`, `:326-351`
- [ ] Fila pista: play, título, "artist · album", encolar, DR, duración (fila no clicable). — `:256-313`

## NowPlayingFullView

- [ ] **Cerrar** (círculo, Escape). — `NowPlayingFullView.swift:36-47`
- [ ] **Fondo**: portada a pantalla + negro 0.5; blur 30 salvo en modo artwork (animado 0.8 s); crossfade 1 s. — `:98-109`
- [ ] **Modos**: lyrics / artwork (por defecto) / visualizer. — `:11`, `:118-134`
- [ ] **Info audio** arriba-derecha: `audioFormat` crudo, kbps, DR sin color. — `:139-153`
- [ ] **Letras**: sincronizadas centradas con línea actual grande, pasadas 0.25, futuras 0.5, autoscroll 0.3 s; texto plano; spinner; sin letras → portada. Clave "artist|title". — `:160-222`, `:21-29`, `:438-463`
- [ ] **Artwork**: imagen r12, chevrons + `"i / N"`, ciclo automático cada 8 s (circular), fade 0.5 s. — `:226-259`, `:381-434`
- [ ] **Visualizer**: Oscilloscope (verde), Spectrum (por defecto, 64 bandas cian), Circular (80 bandas morado). — `:13`, `:263-300`
- [ ] **Barra inferior**: waveform 40 px, tiempos, título, "artist — album", shuffle/prev/play-pause/next/repeat. Sin stop ni seek. — `:304-377`

GTK: blur con `snapshot.push_blur`, `gtk::Stack` crossfade, `glib::timeout_add_seconds_local(8)`.

## LyricsView (panel 320 px)

- [ ] Cabecera "Lyrics" + "Synced" (verde) / "Unsynced" (naranja). — `LyricsView.swift:34-42`
- [ ] Estados: spinner, "Instrumental", "No lyrics available", karaoke, texto plano. — `:51-87`
- [ ] Karaoke: actual title3 negrita, pasadas terciarias, futuras secundarias, autoscroll centrado. — `:103-134`, `:13-25`
- [ ] Fetch al aparecer y al cambiar título; dedupe "artist|title"; nada si "|". — `:138-171`

## WaveformView

- [ ] Canvas solo visual: waveform espejada, Catmull-Rom → Béziers, amplitud `max(1, peak*midY*0.95)`, clip jugado/no jugado; nada si <2 picos. — `WaveformView.swift:3-89`

GTK: `DrawingArea` + cairo o `snapshot` con `gsk::PathBuilder` (≥4.14).

## Visualizadores

- [ ] **FIFOReader**: PCM 16 bit estéreo 44.1 kHz de `/tmp/mpd.fifo`, hilo dedicado, mono, buffer 2048, reabre en EOF, llama a `ensureFifoOutput()`. — `AudioVisualizerView.swift:5-115`
- [ ] **Oscilloscope**: 30 fps, Catmull-Rom, glow de 3 trazos (4/0.3, 2/0.6, 1/1), línea central. — `:120-206`
- [ ] **SpectrumComputer**: Hann, FFT real, `2/n`, bandas log `pow(nyquist, i/bands)`, `sqrt(avg)*4`. — `:212-282`
- [ ] **Spectrum**: gap 12 %, opacidad 0.6+0.4v, cap blanco, picos que caen 0.012/frame, ataque 0.8, caída 0.35. — `:287-383`
- [ ] **Circular**: anillo 0.12×min, barras hasta 0.38×min + espejo, hue 0.75→1.05, glow si >0.4. — `:388-493`
- [ ] **Modos**: Oscilloscope, Spectrum, Circular. — `:497-509`

GTK: `rustfft`/`realfft`, `add_tick_callback` + `queue_draw`; el FIFO es POSIX y funciona igual.
