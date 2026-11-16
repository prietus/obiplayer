# Inventario: núcleo (MPD, estado, biblioteca, ajustes, portadas)

Origen: `~/drplayer/DrPlayer/Sources/`. Mini-player excluido.

Notas generales:
- No hay bucle MPD `idle`: polling cada 1 s. No hay reconexión con backoff.
- La única "normalización de artista" es lowercase + plegado de diacríticos + NFC.
- Único atajo en estos ficheros: Cmd+Shift+M (mini-player, excluido).

---

## 1. Cliente MPD

- [ ] **Una conexión TCP por comando** — conecta, lee saludo `OK`, auth opcional, envía `"<cmd>\n"`, lee hasta `OK`/`ACK`, cierra. Por defecto `localhost:6600`. — `MPDClient.swift:9-64`
- [ ] **Password** — `password <pw>` tras el saludo; `ACK` no se comprueba (password mala falla en silencio). — `:48-61`
- [ ] **ACK tragados** — un error MPD parece resultado vacío; solo fallo de red lanza error. Decodifica UTF-8 cuando el buffer completo es válido. — `:77-106`, `:24-28`
- [ ] **command / commandList** — `command_list_begin … command_list_end`. — `:67-75`
- [ ] **Parsers** — `parseResponse` (split en primer `:`, gana el último); `parsePlaylist` (nuevo registro en cada `file:`; `directory:`/`playlist:` se mezclan en el registro actual). — `:132-142`, `:270-286`
- [ ] **Helpers** — `stats`, `status`, `currentsong`, `playlistinfo`, `listallinfo`, `search any "<q>"`. **Sin escapar comillas** (solo `LibraryFolders` escapa). — `:112-153`
- [ ] **Stickers** — `sticker set/delete/get` (song), `sticker find song "" "<name>"` → mapa uri→valor, `incrementSticker` = get+1+set (no atómico). — `:157-206`
- [ ] **Salidas** — `outputs` → `{id, name, plugin, enabled, attributes}` (`attribute: k=v`); `enableoutput`/`disableoutput`/`toggleoutput`. — `:210-268`

Linux: tokio `TcpStream`. Mantener la semántica; conexión persistente + `idle` sería mejora, no paridad. **Arreglar en el port**: escapar argumentos siempre.

## 2. Estado de reproducción y polling (PlayerViewModel)

- [ ] **Arranque** — `ensureAggregateDevices()` (macOS) → Now Playing → timer 1 s `refresh()` → `refreshOutputs`, `refresh`, `loadLibrary` si vacío. — `PlayerViewModel.swift:141-166`
- [ ] **reconnect()** — recrea cliente con ajustes actuales, resetea versión de playlist, `refresh` + `loadLibrary`. Manual tras cambiar host/puerto. — `:173-181`
- [ ] **Tick `refresh()`** cada 1 s — `:196-297`:
  - [ ] `status` + `currentsong`; `playlistinfo` solo si cambia `status.playlist`. — `:198-209`
  - [ ] estado, título (→ file → `"---"`), artista, álbum, file, pos, `audio`, elapsed, duration, bitrate, repeat/single, random; `connected=true`. — `:213-234`
  - [ ] Lanza DR14 de la pista actual (dedupe por file). — `:227-229`
  - [ ] **Conteo de reproducciones**: al cambiar file se resetea; cuenta 1 cuando `duration>0 && elapsed>50% && play`. — `:236-245`
  - [ ] Waveform al cambiar canción. — `:248-252`
  - [ ] Alimenta `AudioSampleProvider`. — `:255`
  - [ ] Sample-rate matching si activo y reproduciendo; reset en stop. — `:258-268`
  - [ ] Cada 10 ticks: `refreshOutputs()` + `reloadLibraryIfDatabaseChanged()`. — `:271-275`
  - [ ] **Auto-radio al final de la cola**: `stop`, playlist no vacía, el usuario no pulsó Stop → siembra radio desde la playlist y `continueRadio()`. En `play` limpia `userStopped`. — `:278-287`
  - [ ] Now Playing del sistema. — `:290`
  - [ ] Error: `connected=false`, `"Sin conexion a MPD"`. — `:291-296`
- [ ] **RepeatMode** — off(0,0) → queue(1,0) → track(1,1) → off; `repeat N` + `single N` en command list; tooltips "Repeat: off/queue/track". — `:29-55`, `:423-432`
- [ ] **Shuffle** — `random 0|1`. — `:434-438`
- [ ] **Play/pause** — playing → `pause 1`; stop con cola → `play`; si no `pause 0`; limpia `userStopped`. — `:381-392`
- [ ] **Transporte** — `play`, `pause`, `next`, `previous`, `seekcur <int>`, `play <pos>`. **Stop**: `userStopped`, apaga radio y contexto, `stop`. — `:394-443`, `:484-487`
- [ ] **Álbum actual** — por file en tracks → título+artista → solo título. — `:112-124`

Linux: `glib::timeout_add_local` o intervalo tokio + canal al main context.

## 3. Cola

- [ ] **Play album** — `clear`, `add "<folder>"`, `play`, relee `playlistinfo`. — `PlayerViewModel.swift:490-502`
- [ ] **Encolar álbum/pista** — `add`. — `:509-518`
- [ ] **Encolar y reproducir** — `addid` → `Id:` → `playid`. — `:521-530`
- [ ] **Quitar / limpiar** — `delete <pos>`, `clear`. — `:533-542`
- [ ] **Reemplazar cola con lista** — `clear`, un `add` por fichero, `play`. — `:554-561`
- [ ] **Parseo de pista** — requiere `Pos` (id = Pos). Campos: Title (→ file → "?"), Artist, AlbumArtist, Album, file, duration (→ Time), Genre, Date, Track, Disc, Composer, Performer, Conductor, Label, OriginalDate, MUSICBRAINZ_TRACKID, MUSICBRAINZ_ALBUMID, RELEASECOUNTRY. — `:1091-1121`
- [ ] **Track** — incluye `rating` (0–5) y `dr: Int?`. — `:3-26`

## 4. Biblioteca y agrupación en álbumes

- [ ] **loadLibrary(triggerUpdate=true)** — `update` + 2 s opcional; `listallinfo` → álbumes; `playlistinfo`; `libraryIsEmpty` (MPD responde con 0 álbumes); índice de versiones; stickers `preferred`; pasada de GenreEnricher en background con progreso; géneros fusionados ci; cuenta ficheros en `~/.drplayer/dr14` y `waveforms`. Error `"Error cargando biblioteca"`. Termina con `refresh()`. — `PlayerViewModel.swift:323-377`
- [ ] **Recarga automática** — cada ~10 s compara `stats.db_update`; si cambia recarga con `triggerUpdate:false` (no manda `update` para no entrar en bucle). — `:315-321`
- [ ] **Actualizar carpeta** — `update "<path>"`, 2 s, relee, reagrupa. — `:299-311`
- [ ] **DB reconstruida** — notificación `.mpdDatabaseRebuilt` vacía álbumes y recarga. — `:132-138`
- [ ] **Agrupación** — clave = **directorio padre del fichero**; si termina en `.cue` sube otro nivel. Primer track fija: artista = AlbumArtist → Artist → ""; título = Album → carpeta; Date, OriginalDate, Label, CATALOGNUMBER, RELEASECOUNTRY, MUSICBRAINZ_ALBUMID. Géneros y compositores = unión. Ids `"<folder>-<idx>"`. Orden por título localizado ci. — `:1123-1191`
- [ ] **Split de géneros** — sucesivamente por `" / "`, `", "`, `"/"`, `";"`; trim espacios y `*`; capitaliza primera letra salvo palabras ≤2 ya en mayúsculas. — `:1194-1223`
- [ ] **Album** — `totalDuration`; `formattedDuration` `"Xh Ymin"`/`"Y min"`; `format` = extensiones en mayúsculas ordenadas unidas por `" / "`; `avgDR`. — `Album.swift:3-37`

## 5. Agregaciones para navegar

- [ ] **Normalización** — lowercase → plegado diacríticos → NFC. — `PlayerViewModel.swift:1009-1013`
- [ ] **Artistas** — por `album.artist` normalizado; nombre = primero visto; nº álbumes y pistas; orden localizado. Sin split de feat./&, sin "The". — `:1044-1062`
- [ ] **Sellos** — igual por `album.label`; vacío → `"Unknown"`. — `:1023-1042`
- [ ] **Compositores** — por pista con Composer: nº pistas + géneros. — `:1064-1083`
- [ ] **allTracks** — `:1085-1087`

Linux: `unicode-normalization` (NFD, quitar marcas, NFC) + `icu_collator` o comparación lowercase.

## 6. Versiones / versión preferida

- [ ] **Clave de versión** — título lc + sin diacríticos; quitar `(...)`/`[...]` finales repetidos y `- remaster…`; `"<artista>|<título>"`. — `PlayerViewModel.swift:763-778`
- [ ] **Índice** — clave → nº de *otras* versiones; se reconstruye al cargar. — `:744-761`
- [ ] **Preferida** — sticker `preferred=1` en el elegido y borrado en el resto con la misma clave; `preferredFiles` vía `sticker find … preferred`. — `:691-739`

## 7. Valoraciones, reproducciones, historial

- [ ] **Rating** — 0–5, sticker `rating`; `loadRatings(album)` un `sticker get` por pista. — `PlayerViewModel.swift:922-940`
- [ ] **Registrar reproducción** — sticker `play_count` +1, `last_played` = epoch, línea en historial. — `:944-950`
- [ ] **Estadísticas** — `sticker find play_count` + `last_played`, unidas a la biblioteca; solo >0, orden desc. — `:952-989`
- [ ] **Cargas masivas** — `loadAllPlayCounts`, `loadAllFavoriteFiles` (sticker `favorite == "1"`). — `:563-571`
- [ ] **Historial** — `~/.drplayer/play_history.tsv`, append: `ts\tfile\tartist\talbum\ttitle\tdur`; <6 campos se ignora; `entries(since:)`. Sin escapar tabs. — `PlayHistory.swift:5-55`

## 8. Ajustes y configuración (AppSettings)

- [ ] **Claves persistidas** — `AppSettings.swift:7-35`, `:49-72`:
  - `mpdHost`: guardado → `bind_to_address` de mpd.conf → `"localhost"`
  - `mpdPort`: guardado >0 → `port` de mpd.conf → `6600`
  - `musicLibraryPath`: host local → `music_directory` → guardado; remoto → guardado → mpd.conf; último fallback `~/.mpd/music`
  - `mpdPassword`, `hasCompletedSetup`, `lastfmApiKey`, `discogsKey`, `discogsSecret`
  - `sampleRateDeviceUID` (matching activo si no vacío)
- [ ] **Host remoto** — no vacío y no `localhost`/`127.0.0.1`/`::1`. — `:75-80`
- [ ] **Rutas** — `resolvedMusicPath` (symlinks); `resolveFilePath` = `<musicLibraryPath>/<ruta MPD>`. — `:83-90`
- [ ] **Búsqueda de mpd.conf** — `~/.mpd/mpd.conf`, `~/.mpdconf`, `/etc/mpd.conf`, `/usr/local/etc/mpd.conf`, `/opt/homebrew/etc/mpd.conf`; gana el primero con `music_directory`. — `:41-47`, `:269-276`, `:309`
- [ ] **Parser mpd.conf** — por líneas, `#`, match por prefijo (bug: `port` puede casar con claves más largas), quita comillas, expande `~`. Claves: `music_directory`, `bind_to_address`, `port`, `sticker_file`, `follow_outside_symlinks`, `follow_inside_symlinks`, `replaygain`, `auto_update`, `db_file`. Bloques `audio_output { … }` con regex → name, type, device, mixer_type. — `:278-387`
- [ ] **Dependencias** — `which` + `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`, `/bin`, `/opt/local/bin`; ffmpeg, ffprobe, mpd. — `:95-165`
- [ ] **Generar mpd.conf** — `music_directory`, `follow_*_symlinks yes`, playlists/db/state/`sticker.sql`/log en `~/.mpd`, `bind_to_address`, `port`, `auto_update yes`, `replaygain off`; salida `osx` "Default Output" `mixer_type none`; `fifo` "Visualizer" `/tmp/mpd.fifo` `44100:16:2`. — `:170-201`
- [ ] **installMPDConf** — crea `~/.mpd`, `playlists`, music dir; escribe conf; `<musicDir>/.mpdignore` con `*.cue` si no existe. — `:224-243`
- [ ] **ensureFifoOutput** — añade bloque FIFO si no hay `"fifo"`. — `:203-221`
- [ ] **addAudioOutput(DAC)** — (macOS: agregado CoreAudio si nombre ambiguo) + `audio_output { type "osx"; name/device; mixer_type "none"; dop "yes" }`. — `:342-370`
- [ ] **Fuentes de música** — listar entradas top-level (symlinks o dirs); añadir = symlink `<musicDir>/<name>` → destino; quitar solo si es symlink. — `:391-443`

Linux: config TOML en `$XDG_CONFIG_HOME/obiplayer/` o GSettings; búsqueda añade `$XDG_CONFIG_HOME/mpd/mpd.conf`; salidas `type "alsa"` `device "hw:X,0"` (o `pipewire`); `dop "yes"` funciona con ALSA.

## 9. Mapeo de carpetas de biblioteca remota (NFS/SMB)

- [ ] **Concepto** — cada carpeta top-level de la biblioteca MPD remota se mapea a una ruta local con symlink en `~/.drplayer/library/<folder>`; con algún mapeo, `musicLibraryPath` pasa a ser esa raíz. — `LibraryFolders.swift:3-8`, `:325-327`
- [ ] **fetchFolders** — `lsinfo`; vacío + `stats.songs` → `[]`, si no `nil` (inalcanzable). Por cada `directory:` busca fichero muestra y lista entradas directas. — `:30-48`
- [ ] **Fichero muestra** — DFS por `lsinfo`, profundidad ≤6, primeros 3 subdirs por nivel. — `:50-62`
- [ ] **Escapado MPD** — `\` → `\\`, `"` → `\"`. — `:64-66`
- [ ] **map / target** — `:76-88`
- [ ] **Test de match** — `<local>/<muestra sin "<folder>/">` existe. — `:91-95`
- [ ] **Cobertura** — cuántas entradas directas existen localmente (detecta copias incompletas). — `:99-104`
- [ ] **Raíces candidatas** — montajes nfs/smbfs/afpfs/webdav/autofs + `/Volumes/*`. — `:109-137`
- [ ] **Directorios de búsqueda** — raíces + 2 niveles, sin ocultos, ≤40 hijos/dir. — `:140-156`
- [ ] **detect** — prueba `<dir>/<folder>` y luego `<dir>`; mayor cobertura; para en completa. — `:161-178`
- [ ] **Vista LibraryFolders** — debounce 500 ms en host:port; mensajes "Enter the server address first", "Could not read folders from MPD at X", biblioteca vacía; arrastre automático de carpetas bajo la base anterior; estados completo/incompleto "N of M missing"/incorrecto/sin asignar; "Choose…"; "Detect all" con "Found X of Y" / "N not found — mount the share or use Choose…" / "All folders are already set". — `:183-362`

Linux: `/proc/self/mounts` (nfs, nfs4, cifs, smb3, fuse.sshfs, davfs, autofs, fuse.gvfsd-fuse) + `/media/$USER/*`, `/run/media/$USER/*`, `/mnt/*`, `$XDG_RUNTIME_DIR/gvfs/*`; `gtk::FileDialog::select_folder`.

## 10. Portadas

- [ ] **findCover** (ruta resuelta) — `Album.swift:117-209`:
  - Extensiones jpg, jpeg, png, bmp, tiff, tif, webp; nombres `cover`, `folder`, `front`, `artwork`, `cover_for_foobar`.
  - Paso 0: `exists` directo nombre × ext (minúscula y Capitalizada) — evita listar dir (NFS).
  - Paso 1: primera imagen del listado (sin ordenar).
  - Paso 3: subcarpetas `Art`, `Artwork`, `Scans`, `Covers` (+ minúsculas): nombre preferido, si no primera.
  - Paso 4: hasta 3 padres mientras la carpeta empiece por `disc`, `cd`, `stereo`, `mono`, `surround`, `side`, `disk`.
- [ ] **coverImage (sync)** — caché en memoria (nil cacheado = hit). — `:42-53`
- [ ] **coverImageAsync** — local → `CoverArtService.downloadCover`; fallo cachea nil. — `:56-66`
- [ ] **allArtwork (galería)** — imágenes de la raíz (ordenadas) + Art/Artwork/Covers/Scans; carpetas disc/cd/… también padre. — `:69-115`
- [ ] **Descarga online** — `CoverArtService.swift:18-57`:
  1. `~/.drplayer/covers/<key>.jpg`; `<key>.notfound` → nil
  2. Discogs (si hay key): con MBID → catno de MB → Discogs por catno; si no por artista + título limpio. — `:61-86`
  3. Cover Art Archive `https://coverartarchive.org/release/<MBID>/front`. — `:90-94`
  4. Búsqueda MB (artista + título limpio) → CAA. — `:96-103`
  - `.notfound` solo si hay `discogsKey`.
- [ ] **HTTP** — UA `DrPlayer/1.0 (music player)`, 200–399, cuerpo **>1000 bytes**, sigue `Location`. — `:107-123`
- [ ] **Clave** — djb2 de `"artist|album"` lc. — `:127-132`
- [ ] **clearCache** — `:156-162`
- [ ] **Limpieza de título** — comillas, `^\d{4}\s+`, `(...)`/`[...]` finales. — `:166-179`
- [ ] **CoverCache** — carpeta → imagen con 3 estados (ausente/imagen/negativo); `invalidate`; sin límite. — `CoverCache.swift:4-32`
- [ ] **ArtworkThumbnail** — carga off-main, 160 px alto, radio 6, sombra; placeholder 120×160 con spinner; click → popover ≤600×600 con nombre. — `ArtworkThumbnail.swift:3-52`

Linux: `gdk::Texture`/`gdk_pixbuf` (webp requiere webp-pixbuf-loader) o crate `image`; `reqwest`.

## 11. Caché de metadatos

- [ ] **MetadataCache** — `~/.drplayer/metadata/<djb2>.cache`, TTL por mtime (30 días, configurable por llamada); Data/String/JSON. — `MetadataCache.swift:4-67`
- Otros dirs: `~/.drplayer/{dr14,waveforms,covers,library,genres,artist-avatars}`, `play_history.tsv`.

Decisión pendiente: mantener `~/.drplayer` (compartir cachés) o `$XDG_CACHE_HOME/obiplayer`.

## 12. Smart playlists

- [ ] **Modelo** — campos Genre, Artist, Label, Format, Year, DR, Play Count, Rating, Composer, Country; numéricos Year/DR/Play Count/Rating. Operadores numéricos is/>/</between; texto contains/doesn't contain/is/is not. all/any. `limit` opcional. Nueva: "New Playlist" + regla Genre contains "". — `SmartPlaylist.swift:5-81`
- [ ] **Persistencia** — JSON en UserDefaults `smartPlaylists`; raw values como strings (`"Play Count"`, `"doesn't contain"`). — `:85-97`
- [ ] **Evaluación** — todos los (track, album) en orden de biblioteca, limit = primeros N. Genre = split `/` + `album.genres`; Format especial `DSD` = DSF|DFF (con operador distinto de `is` se niega); Year = 4 primeros chars de Date; between inclusivo. — `:102-196`

## 13. Radio

- [ ] **Semillas** — álbum: géneros, artista, década, primer compositor, sello. Pista: géneros. Playlist: últimas 5 pistas, unión de géneros, artista/década/compositor más frecuentes. — `RadioEngine.swift:17-97`
- [ ] **Stop tags** — `classic`, seen live, favorite(s)/favourite(s), awesome, amazing, good, great, best, love(d), albums i own, owned, own, music, song(s), décadas `00s`…`90s`, `1900s`…`2020s`. — `:34-51`
- [ ] **Década** — ≥4 chars y año >1900 → `(año/10)*10`. — `:236-239`
- [ ] **Puntuación** — `:102-187`:
  - Filtro de afinidad: similar>0, Jaccard>0, compositor o mismo artista.
  - similar `int(match*20)`; `int(jaccard*genreMax)` (25 con Last.fm, 50 sin); misma década +3, adyacente +1; compositor +8; rating×3; mismo artista +2.
- [ ] **Diversidad** — orden + shuffle + orden estable con buckets de 3 puntos; máx. `max(2, n/10)` por artista y 2 por álbum; segunda pasada sin tope de álbum. — `:190-231`
- [ ] **Flujos** — `PlayerViewModel.swift:576-628`: desde álbum (clear, add, play, generar); desde actual; generar 20 pistas (Last.fm similares, excluye cola); continuar = clear + regenerar desde la **semilla original**; stop; auto-start (§2).

## 14. Ganchos DR14 / waveform / visualizador

- [ ] **DR de la pista actual** — se aplica solo si sigue siendo la actual. — `PlayerViewModel.swift:634-648`
- [ ] **Escaneo DR de álbum** — salta si todas tienen DR; en orden; actualiza `avgDR` con `albumDR`. — `:651-686`
- [ ] **Waveform** — se descarta si cambió la canción. — `:908-918`
- [ ] **Visualizador** — `AudioSampleProvider.update` cada tick. — `:255`

## 15. Sample-rate matching y DAC

macOS: todo esto es CoreAudio. Inventario de lo que hace:
- [ ] **Parseo de formato** — PCM `"44100:24:2"`; DSD `"dsdN:…"` → portadora DoP `44100 × N/64 × 4`. — `PlayerViewModel.swift:809-826`
- [ ] **Secuencia de cambio** — anti-reentrada; ±1 Hz = ya ok; salida MPD a ciclar = la que se llama como el DAC o con `device` == DAC, si no la primera activa; si no es la primera pista: `disableoutput`, 500 ms, fijar rate (reintento), 500 ms, `enableoutput`. — `:832-904`
- [ ] **Agregados al arrancar** — `:784-796` (no aplica en Linux)
- [ ] **Enumeración de dispositivos** — nombre, fabricante, transporte, canales, rate nominal, rangos, UID, max rate físico, bit depths, formato actual. — `CoreAudioDevices.swift:4-132`, `:150-223`
- [ ] **formatRate** — `"N kHz"`/`"%.1f kHz"`/`"N Hz"`. — `:225-233`
- [ ] **Nivel de calidad** — ≥705600 "DSD512", ≥384000 "DSD / DXD", ≥96000 "Hi-Res", ≥48000 "CD+", si no "CD". — `:242-250`

Linux: enumerar con ALSA (`/proc/asound`, hw_params de `hw:`) o PipeWire (`pw-dump`). Con salida MPD `alsa` en `hw:X,0` el cambio de rate es nativo y bitperfect: **no hace falta** el ciclado ni agregados. Con PipeWire: `clock.allowed-rates` / `pw-metadata -n settings 0 clock.force-rate` + el mismo ciclado de salidas.

## 16. Now Playing / teclas multimedia

- [ ] **Comandos** — toggle/play/pause (todos → toggle), next, previous, seek → `seekcur`. — `NowPlayingBridge.swift:17-66`
- [ ] **Actualización** — título, artista, álbum, duración, elapsed, rate, portada (solo al cambiar file). — `:69-103`, `PlayerViewModel.swift:449-482`
- [ ] **Limpiar en stop** — `NowPlayingBridge.swift:106-111`

Linux: **MPRIS2** (`mpris-server`/zbus): `xesam:title/artist/album`, `mpris:length` (µs), `mpris:artUrl` `file://` de portada cacheada, PlaybackStatus, Position, `Seeked`, PlayPause/Play/Pause/Next/Previous/SetPosition. (Aquí Play/Pause pueden ser correctos en vez de toggle.)

## 17. Shell de la app

- [ ] **Ventana** — mínimo 600×500, por defecto 900×700. — `DrPlayerApp.swift:9-25`
- [ ] **About** — nombre, versión, créditos ("A music player for people who care about editions…", SwiftUI/MPD/MusicBrainz/Discogs/Last.fm/Wikipedia, GPLv3, URL GitHub), icono. — `:26-40`, `:47-61`
- [ ] **Preferencias** — ventana aparte (Cmd+,). — `:42-44`
- [ ] **Esquema URL** — `drplayer://identify?album=<path>&discogsId=<int>` → `PressingStore.certify`. — `:12-23`
- [ ] **Activación** — política regular (macOS, no aplica).
- [ ] **ArtworkServer (puente OBIScanner)** — HTTP localhost:**18923**, sirve un fichero (no parsea la petición), MIME por extensión o 404; deep links `obiscanner://scan?server=…&path=…` y `obiscanner://identify?…&album=…&callback=drplayer`. — `ArtworkServer.swift:6-108`

Linux: `adw::Application` + `adw::ApplicationWindow`, `adw::AboutDialog` (GPL-3.0), menú primario `gio::Menu`, `<Ctrl>comma`, `adw::PreferencesDialog`, `.desktop` con `x-scheme-handler`, `HANDLES_OPEN`. OBIScanner probablemente es macOS/iOS: **decidir si el puente tiene sentido en Linux.**

## 18. Temas

- [ ] **Temas** — Classic (azul acero `0.35,0.55,0.75` / cian `0.4,0.6,0.7` / ámbar `0.7,0.55,0.3`), Warm (`0.75,0.55,0.3` / `0.65,0.45,0.3` / `0.8,0.6,0.25`), Mono (gris 0.7/0.55/0.85), Vinyl (`0.3,0.55,0.4` / `0.35,0.5,0.4` / `0.7,0.65,0.5`); paleta de 6 colores para gráficos; colores DR fijos verde/amarillo/naranja/rojo. — `AppTheme.swift:5-86`
- [ ] **Persistencia** — `appTheme`, por defecto `"Classic"`. — `:92-102`

Linux: CSS provider sobre `@define-color accent_bg_color`/`accent_color`; `adw::StyleManager` ForceDark.

## 19. Cadenas

- Errores en español hardcoded (`"Sin conexion a MPD"`, `"Error cargando biblioteca"`); resto en inglés con `Localizable.strings` en/es. — `PlayerViewModel.swift:294`, `:372`

Linux: `gettext-rs` + `.po`.
