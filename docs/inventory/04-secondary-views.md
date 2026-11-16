# Inventario: vistas secundarias, ajustes y asistente

Origen: `~/drplayer/DrPlayer/Sources/`. Colores DR/formato y barra DR: ver `03-main-views.md`.

---

## ArtistListView

**Lista**
- [ ] Búsqueda "Search artists..." con ×, "N artists"; filtro ci por nombre; orden heredado del VM. — `ArtistListView.swift:11-49`
- [ ] Fila: avatar circular 36 (`ArtistAvatarService`, placeholder persona), nombre, "N albums · N tracks", chevron. — `:73-121`

**Detalle** (`ArtistDetailPanel`, `:125-560`)
- [ ] "‹ Artists". — `:175-187`
- [ ] Foto 160×160 (clic → popover ≤600); placeholder 140. — `:236-257`, `:315-324`
- [ ] Chips MB: tipo, área, periodo ("since X" / "X – Y"). — `:264-273`
- [ ] Tags: hasta 6 de Last.fm, si no géneros de la biblioteca. — `:276-290`
- [ ] Oyentes y reproducciones Last.fm en K/M. — `:293-305`, `:517`
- [ ] Enlaces Wikipedia, Last.fm, MusicBrainz. — `:336-361`
- [ ] "Loading...". — `:193-201`
- [ ] Biografía (Wikipedia → Last.fm, "Source: X"), Miembros (MB), Similares (Last.fm, " · "). — `:366-420`
- [ ] "In your library": Albums, Tracks, duración, periodo min–max, formatos, DR medio + barra. — `:433-479`
- [ ] "Discography (N albums)" con `AlbumGridView`. **Bug**: al seleccionar álbum el padre recibe el artista, no el álbum. — `:213-224`, `:22`
- [ ] Carga: nombre de búsqueda vía `artistSearchQueries(name).first` → MB `searchArtist` ∥ Last.fm `fetchArtist` → Wikipedia por slug MB o `searchArtist` → avatar. — `:533-558`

## ComposerListView

- [ ] Lista: "Search composers...", ×, "N composers"; fila: icono, nombre, "N tracks", hasta 3 géneros. — `ComposerListView.swift:11-99`
- [ ] Pistas del compositor: `composer` normalizado (lc + sin diacríticos). — `:21-26`
- [ ] Detalle: "‹ Composers", foto 140 (popover), chips MB, hasta 6 géneros de la biblioteca, stats Last.fm, enlaces. — `:136-288`
- [ ] Biografía y similares (sin miembros). — `:314-343`
- [ ] "In your library": Tracks, Albums, duración, Performers (primeros 5). — `:361-389`
- [ ] "Compositions (N tracks)": play, título, "artist · album", DR, duración. — `:400-452`
- [ ] Foto solo desde thumbnail de Wikipedia (sin AvatarService). — `:500-514`

## ProducerListView

- [ ] **Construcción**: por álbum, release MB **solo desde caché** (`fetchReleaseFromCache(mbid)` o `fetchCachedRelease`); créditos con rol que contenga producer, executive producer, mastering, engineer, recording, mix, balance, remastered, remixer; mapa nombre → [(álbum, roles)]; publicación incremental con "Scanning X/Y...". — `ProducerListView.swift:187-230`
- [ ] Búsqueda "Search producers...", "N producers". — `:86-112`
- [ ] Chips de rol: "All" + roles base (antes de "(", capitalizados). — `:115-126`, `:169-183`, `:42-48`
- [ ] Orden por nº de álbumes desc. — `:39-53`
- [ ] Vacío: "No producer data found" / "Producer credits come from MusicBrainz.\nBrowse albums first to cache metadata." — `:130-145`
- [ ] Fila: avatar 36, nombre, "N albums · roles". — `:235-290`
- [ ] Deep link `initialProducer`. — `:7`, `:74-79`
- [ ] Detalle: "‹ Producers", avatar 120, roles, "N albums in library", Wikipedia 6 líneas ("Loading bio..."), grid con rol bajo cada álbum → abre álbum. Con deep link sin datos, escanea la biblioteca. — `:294-462`

## LabelListView

- [ ] Lista: "Search labels...", "N labels"; fila: icono, nombre, "N albums · N tracks". — `LabelListView.swift:30-91`
- [ ] Detalle: "‹ Labels", nombre, recuentos; "Years: a – b", "Avg DRn", "Formats", "Artists" (10 primeros + "..."). — `:137-186`
- [ ] Wikipedia: `"<label> record label"` → `label`; 6 líneas + "Read more on Wikipedia". — `:189-221`
- [ ] Géneros (hasta 20, mayúsculas, flow). — `:224-237`
- [ ] Álbumes por fecha asc: portada 48, título, artista · fecha, badge formato, DR, "N tracks". **Bug**: el clic pasa el sello, no el álbum. — `:243-300`, `:22`

## StatsView

- [ ] Estados: "Loading play history..."; vacío "No play data yet" / "Stats will appear here as you listen to music". — `StatsView.swift:121-141`
- [ ] Datos: mapas duración/género/sello por fichero; `vm.loadPlayStats()` (stickers) + `PlayHistory.loadAll()`; carga una vez. — `:174-193`
- [ ] **Periodo**: Week / Month / Year / All Time (por defecto). All Time = stickers (plays × duración); resto = historial. — `:14-30`, `:197-261`
- [ ] **Hero**: tiempo de escucha `Xh Ym`, "Total listening time" / "<Periodo> listening time", chips plays/tracks/artists/albums. — `:218-247`
- [ ] **Géneros**: donut top 6 (radio interior 60 %, empieza en −90°, `chartColors`), total en el centro, leyenda. — `:279-351`
- [ ] **Top Artists** (5, avatar, barra, duración). — `:355-392`
- [ ] **Top Albums** (5, portada, barra). — `:396-435`
- [ ] **Top Labels** (5; "No label data"). — `:439-483`
- [ ] **Day of Week**: 7 barras L–D, altura h/max×80, "Avg X.Xh/day". — `:487-520`, `:96-106`
- [ ] **Listening Hours**: 24 barras, opacidad 0.3+0.7×i, ejes 0/6/12/18, "Peak listening: H:00". — `:524-560`
- [ ] **Top Tracks**: 10 (siempre all-time), rango, título, "artist — album", nº, barra; clic reproduce. — `:564-591`
- [ ] **Recently Played**: 8 por `lastPlayed`, fecha relativa; clic reproduce. — `:595-620`

GTK: `.card`, `DrawingArea` + cairo, fechas relativas propias o `chrono-humanize`.

## DRComparisonPopover

- [ ] Popover 480: "Dynamic Range", "Your version: DRn". — `DRComparisonView.swift:15-28`, `:99-100`
- [ ] "Querying Loudness War DB..." / "No data found in Loudness War DB for this album". — `:30-42`
- [ ] Tabla Edition, Year, DR, Min, Max, Codec, Source (máx. 300 alto); Edition abre `/album/view/<id>`; fondo acento 8 % igual, verde 4 % mayor, rojo 4 % menor. — `:44-76`, `:105-159`
- [ ] Pie "Source: dr.loudness-war.info" + "Open in browser" (`/album/list?artist=&album=`). — `:79-97`

## EditionComparisonView

- [ ] Cabecera: back con título del álbum, "Compare editions", "artist — title" + DR (avgDR → calculado → media de pistas). — `EditionComparisonView.swift:22-62`
- [ ] 3 tareas concurrentes: Loudness War, release info, scan DR. — `:80-88`
- [ ] **Mis versiones**: misma regla de matching que AlbumDetail (incluye la actual); ranking por `AudioQualityScore` de la primera pista. — `:103-127`, `:498-509`
  - [ ] Fila 1: play (primera pista), título (negrita si mejor), "BEST" (si >1 edición), "CURRENT", "Npts". — `:152-189`
  - [ ] Fila 2: formato, DR + barra (o "DR …"), "N tracks", duración. — `:192-227`
  - [ ] Fila 3: Label (API solo si esa fuente tiene catno; si no tag → MB → Discogs; **clicable** → sello), Catalog, Barcode, Country, Date, Original, Source, Producer/Engineer/Mastering. — `:239-349`
  - [ ] Fila 4: carpeta mono 9 pt truncada por delante. — `:260-265`
  - [ ] Fondo verde si mejor, rojo si warning. — `:269-272`
  - `onSetPreferred`/`preferredFiles` declarados pero sin usar.
- [ ] **Loudness War DB**: título → `/album/view/<id>`, DR (min–max), badge codec (azul si "lossless", si no naranja), source, año, label/catno/país (detalles en paralelo, actualización in situ); fondo acento/verde (sin rojo). — `:353-494`, `:561-581`
- [ ] **Scan DR**: por edición sin avgDR, pistas cacheadas o `DR14Analyzer.analyze`; media; `onScanDR` persiste. — `:583-626`
- [ ] **Release lookup**: `cleanTitle` (+ deluxe/bonus), `extractCatalog` (regex tipo `(ABC-123)`, `(123 ABC)`, ignora remaster/deluxe/live/edition); cascada MB y Discogs como AlbumDetail. — `:541-702`

## SmartPlaylistView

- [ ] Split: barra lateral 180–220 | editor + resultados; placeholder "Select or create a smart playlist". — `SmartPlaylistView.swift:22-51`
- [ ] Lista: icono varita morado; menú contextual **Rename** (inline, Enter) y **Delete**; cambiar selección evalúa; "+ New Playlist". — `:56-110`
- [ ] Editor: nombre, **"Play All (N)"** → `enqueueAndPlayAll`. — `:117-133`, `:374-379`
- [ ] "Match [all|any] of the following rules:". — `:136-150`
- [ ] Regla: campo (resetea operador), operador, valor (oculto si booleano), "to" en `between`, borrar si >1 regla. — `:191-251`
- [ ] "+ Add Rule" (no reevalúa — bug menor). — `:158-166`
- [ ] "Limit to" (50 por defecto) "tracks". — `:169-186`
- [ ] Cada cambio guarda y evalúa en vivo. — `:359-372`
- [ ] Resultados: "N tracks matched" / "No tracks match these rules"; columnas play | Title | Artist 150 | Album 150 | Label 100 | DR 40 | Dur. 50. — `:255-355`

GTK: `gtk::Paned`, `adw::ToggleGroup`, `gtk::CheckButton`, `PopoverMenu`.

## SignalPathIndicator / Popover

- [ ] **Regla bitperfect** (exacta): primera salida MPD activa con `mixer_type == "none"` **o** sin atributos. No mira replaygain, resampling, volumen ni DoP. — `SignalPathView.swift:9-13`
- [ ] **Color**: gris (no conectado/no reproduciendo), verde (rate matched + bitperfect), cian (bitperfect), morado (resto). — `:15-19`
- [ ] Indicador: punto 8 px (5 compacto), glow; tooltips "Bitperfect · Rate matched" / "Bitperfect" / "Signal path". — `:21-38`
- [ ] Popover 280: "Signal path" + pills "Rate Matched"/"Bitperfect"; cadena: — `:46-126`
  1. Origin: `"44.1 kHz / 24 bit / stereo"` / `"DSD NNN stereo"` / `"---"`. — `:162-188`
  2. MPD: "N kbps · No processing" / "· Active processing". — `:190-197`
  3. Rate Match → rate (si matched): dispositivo, "DSD<n> (DoP)" o rate. — `:93-102`, `:212-220`
  4. Output: nombre, PLUGIN, "no mixer"/"mixer: X", "DoP"; o "No active output". — `:106-121`, `:199-210`

Linux: considerar endurecer la regla en Linux (comprobar `replaygain`, `auto_resample`, `auto_format`, `device` hw:) — decisión pendiente, sería mejora sobre paridad.

## BackgroundTasks

- [ ] Indicador engranaje, naranja + punto si `genreEnrichRunning`; "Background tasks". — `BackgroundTasksView.swift:4-33`
- [ ] Popover 340, tareas: Genre enrichment ("Processing done/total albums..." + barra / "Completed · N albums cached"), DR14 ("On demand · N tracks cached"), Waveforms (ídem), Audio outputs ("N outputs · polling every 10s"), Radio (si activa: "Active · auto-continue when queue ends"); pie con rutas de caché. Sin acciones. — `:36-155`

## SettingsView

Ventana 560×400, pestañas General / Sources / Audio / Appearance; guardado inmediato. — `SettingsView.swift:7-29`

**General**
- [ ] **Biblioteca**: local → ruta + "Choose..." + "Auto-detected from <conf>"; remoto → `LibraryFoldersView` embebido + texto explicativo. — `:70-97`
- [ ] **Conexión MPD**: Host, Port (1–65535), Password; **Test connection** (`status()`; "Connected" / "Unexpected response" / "Could not connect"; no envía password — bug); nota de MPD remoto. — `:99-149`, `:368-385`
- [ ] **APIs externas**: Last.fm key + "Test" (`validateKey`; "Valid" o error); Discogs key + secret + "Test" (`validateCredentials`); ayudas con URLs de alta. — `:151-216`
- [ ] **Salud de mpd.conf** (si se detecta): ruta + filas — `:219-254`

  | Clave | OK si | Detalle si no |
  |---|---|---|
  | `sticker_file` | definido | "missing — preferred versions won't persist" |
  | `follow_outside_symlinks` | yes | "no — music sources via symlinks won't work" |
  | `follow_inside_symlinks` | yes | "no — nested symlinks won't resolve" |
  | `replaygain` | off o sin definir | "not set (bitperfect)" |
  | `auto_update` | siempre | "no — run `mpc update` manually after adding music" |
  | `db_file` | definido | "not set" |

- [ ] **Mantenimiento** — `:256-473`:
  - [ ] **Rebuild MPD Database** con confirmación ("This will stop MPD, delete its database, and restart it… stickers are preserved."): parar MPD → borrar `db_file` → arrancar MPD → "Database rebuilt — library will reload automatically" + notificación `.mpdDatabaseRebuilt`.
  - [ ] **Purge Metadata Caches**: borra `~/.drplayer/{metadata,genres,covers}` (sin confirmación ni feedback).
- [ ] Al aparecer: `detectFromMPDConf()`. — `:317-321`

**Sources**
- [ ] Remoto: aviso "MPD is running on <host>… Symlinks created on this Mac are not visible to a remote MPD." — `:490-509`
- [ ] Local: lista de fuentes (symlink o dir) con destino; quitar (solo symlinks); "Add source..." → symlink; tras cambiar, `update`; errores "Error: …"; pie explicativo. — `:478-616`

**Audio**
- [ ] **Dispositivos** (CoreAudio): tarjeta con icono por transporte, fabricante, badges de transporte y calidad, Active / Channels / Bit depth / Max rate / Supported rates; USB: "Add to mpd.conf" o "Configured in mpd.conf"; aviso de conflicto de nombre; tras añadir, banner "Restart MPD to apply changes". — `:706-827`
- [ ] **Sample Rate Matching**: selector de DAC ("Disabled" + USB), estado, aviso de `mixer_type "none"`. — `:871-916`
- [ ] **Salidas MPD**: nombre, chip plugin, atributos, "bitperfect", switch → `toggleOutput`; estados carga/error/vacío; pie. — `:920-974`

**Appearance**
- [ ] Tema (radio) + muestras de color. — `:979-1022`

Linux: `adw::PreferencesDialog`, `adw::PasswordEntryRow` (+ libsecret), `adw::AlertDialog`, `gtk::FileDialog`; MPD vía `systemctl --user stop/start/restart mpd` con fallback `pkill -x mpd` / `mpd`; dispositivos vía ALSA (`/proc/asound`, `usbid`, udev `ID_BUS`) o PipeWire; salida generada: `type "alsa"`, `device "hw:CARD=X,DEV=0"`, `mixer_type "none"`, `auto_resample "no"`, `auto_format "no"`, `dop "yes"`. Sin agregados.

## FirstRunSetupView (asistente)

Formulario único 560×560 con secciones progresivas. Init: `detectFromMPDConf()` + `checkDependencies()`. — `FirstRunSetupView.swift:29-121`

- [ ] **1. Where does MPD run?** — "On this Mac" / "On another machine"; cambiar limpia test y ajusta host/ruta. — `:125-149`
- [ ] **2. Prerequisites** — mpd, ffmpeg, ffprobe con ruta o "not found"; remoto: mpd "not needed (remote server)"; si falta algo: `brew install …` + "Verify again"; **el resto se oculta hasta que estén**. — `:153-211`, `:25-27`, `:60-69`
- [ ] **3. MPD Configuration** (local) — "mpd.conf detected" + ruta, o "mpd.conf not found" + `music_directory` + **Generate mpd.conf** ("Created at ~/.mpd/mpd.conf"). — `:215-268`
- [ ] **Conexión** — host ("Server IP or hostname" si remoto), puerto, `music_directory` si local con conf. — `:272-293`
- [ ] **4. Library folders on this Mac** (remoto) — `LibraryFoldersView`. — `:297-304`
- [ ] **5. Music sources** (local) — lista, "Add source..." (symlink), vacío "No sources. You can add them now or later in Settings.". — `:308-408`
- [ ] **Pie** — Test connection, aviso "No music sources added", **Start** (deshabilitado según modo); `saveAndComplete`: guarda, `hasCompletedSetup = true`, `update` si local con fuentes. — `:74-116`, `:432-448`

mpd.conf generado: ver `01-core.md` §8. Linux: pista de instalación por distro (`apt`/`dnf`/`pacman`), "On this computer", salida `alsa`/`pipewire`, opción `systemctl --user enable --now mpd`.

## LibraryFoldersView

Detalle completo en `01-core.md` §9. Estados: "Reading library folders from MPD…", errores con **Retry**; fila con icono de estado (completo / "N of M missing" / incorrecto / sin asignar), "Choose…" ("Where is the “X” library on this Mac?"), **Detect all** ("Searching network mounts…").

## Localización

- 147 claves en/es, pero la mayoría de cadenas visibles son literales en inglés sin entrada; claves con `%d` vs `%lld`; textos que no coinciden; probablemente el español no carga en el build SwiftPM.
- Port: **gettext desde el principio**, con todas las cadenas marcadas; traducción es completa (con tildes).
