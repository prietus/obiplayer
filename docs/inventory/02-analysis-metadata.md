# Inventario: análisis de audio y servicios de metadatos

Origen: `~/drplayer/DrPlayer/Sources/`. Las referencias `Fichero.swift:línea` apuntan a ese árbol.

Helpers compartidos:
- **MetadataCache** (`MetadataCache.swift`): `~/.drplayer/metadata/<djb2hex(key)>.cache`, TTL 30 días por mtime. Texto UTF-8. Centinelas `"(none)"` / `"(empty)"` = resultado negativo cacheado.
- **djb2**: `h=5381; h = h*33 + byte` (UInt64 wrapping) sobre bytes UTF-8 → hex minúsculas sin padding. Mantener idéntico si se quieren reutilizar cachés existentes.

---

## 1. DR14

- [ ] **analyze(filePath)** — resuelve symlinks, nil si no existe, mira caché, decodifica con ffmpeg, calcula, cachea solo éxitos. — `DR14Analyzer.swift:29`, `:49`
- [ ] **Downmix mono** — `ffmpeg -ac 1` antes de analizar; NO es DR por canal promediado como el spec oficial (puede diferir de foobar/TT DR). — `DR14Analyzer.swift:134-143`
- [ ] **Sample rate** — `ffprobe -v quiet -select_streams a:0 -show_entries stream=sample_rate -of default=noprint_wrappers=1:nokey=1 <path>`; aborta si falla o 0. — `DR14Analyzer.swift:159-179`
- [ ] **Decodificación** — `ffmpeg -i <path> -ac 1 -f f32le -acodec pcm_f32le -ar <rate> -v quiet -y <tmp>`; Float32 nativo; falla si <4 bytes. — `DR14Analyzer.swift:125-157`
- [ ] **Bloques** — `blockSize = sampleRate*3`, `blockCount = n / blockSize` (descarta bloque parcial); <1 bloque → nil (pistas <3 s sin DR). — `DR14Analyzer.swift:24`, `:68-72`
- [ ] **RMS por bloque** — `sqrt(2 * Σx² / N)`. — `:84-86`
- [ ] **Pico por bloque** — max |x|. — `:89-91`
- [ ] **Top 20 % RMS** — ordenar asc, últimos `max(1, blockCount/5)`; media aritmética lineal. — `:96-99`
- [ ] **Pico** — segundo mayor pico de bloque (sin deduplicar); si 1 bloque, el mayor. — `:102-108`
- [ ] **DR** — `-20*log10(meanRMS/secondPeak)`, redondeo half-away-from-zero, clamp 0..99; nil si ≤0. — `:110-113`
- [ ] **Peak/RMS dBFS** — `20*log10(secondPeak)`, `20*log10(meanRMS)`. — `:115-116`
- [ ] **albumDR** — ignora ≤0, media entera truncada; nil si vacío. — `:59-63`
- [ ] **Caché** — `~/.drplayer/dr14/<djb2hex(resolvedPath)>.dr14`, texto `"dr,peak,rms"` (3 partes). Sin TTL ni invalidación por mtime. — `:18-22`, `:183-209`
- [ ] **Runner** — `posix_spawn` + `waitpid`, éxito = status 0. — `:213-251`
- [ ] **Localización de ffmpeg/ffprobe** — `AppSettings.findBinary` con lista de directorios conocidos (`AppSettings.swift:134-135`); sin binario → nil.

Linux: vDSP → iteradores Rust; `posix_spawn` + tmp → `std::process::Command` leyendo stdout directo; `resolvingSymlinksInPath` → `fs::canonicalize`; colas → `spawn_blocking`/`rayon`.

## 2. Muestras para visualizador (FIFO / fichero)

- [ ] **Modo FIFO** — solo si existe `/tmp/mpd.fifo` y `mpdHost` es exactamente `localhost` o `127.0.0.1`. — `AudioSampleProvider.swift:18-22`, `AppSettings.swift:203`
- [ ] **start(file)** — FIFO → `FIFOReader.shared`; si no, `FileAudioReader.prepare` en tarea aparte. — `AudioSampleProvider.swift:25-34`
- [ ] **update(file, elapsed, playing)** — guarda elapsed + timestamp bajo lock. — `:37-52`
- [ ] **Interpolación** — playing: `lastElapsed + (now - lastUpdate)`; pausa: `lastElapsed`. — `:55-61`
- [ ] **getSamples / getAccumulatedSamples** — FIFO delega; fichero llama `FileAudioReader.update(elapsed)` cada frame. — `:63-78`
- [ ] **reset()** — al cambiar host MPD. — `:81-90`
- [ ] **prepare** — ruta MPD → `AppSettings.resolveFilePath` → symlinks; flag global `decoding` (un fichero nuevo se descarta si hay otro decodificando); requiere fichero local. — `FileAudioReader.swift:23-37`
- [ ] **Decodificación** — `ffmpeg -y -i <full> -ac 1 -ar 44100 -f f32le -acodec pcm_f32le $TMPDIR/drplayer_viz.raw`; fallback hardcoded `/opt/homebrew/bin/ffmpeg`; pista entera en memoria. — `:40-104`
- [ ] **Ventana** — `start = elapsed*44100`, `[start, start+2048)`; fuera de rango → vacío. — `:107-129`
- [ ] **clear()** — `:143-150`

Linux: `NSLock` → `Mutex`; `CFAbsoluteTimeGetCurrent` → `Instant`; decodificar en proceso con `symphonia` es opcional.

## 3. TrackProbe (ffprobe)

- [ ] **probe(path)** — `ffprobe -v quiet -print_format json -show_format -show_streams <resolved>`. — `TrackProbe.swift:5-12`, `:55-75`
- [ ] **Stream** — primer `codec_type=="audio"`: `codec_name`, `codec_long_name`, `sample_rate`, `channels`, `channel_layout`, `sample_fmt`, `duration`. — `:80-96`
- [ ] **Bit depth** — `bits_per_raw_sample` si ≠ "0", si no `bits_per_sample` si >0, si no vacío. — `:89-93`
- [ ] **Format** — `format_long_name` → `format_name`; `size`, `bit_rate`; duration fallback a format; `tags` crudos. — `:99-113`
- [ ] **readFileTags(path)** — `ffprobe -v quiet -print_format json -show_entries format_tags`; claves en MAYÚSCULAS; `{}` si falla. — `:16-53`
- [ ] **Errores** — ignora exit status; fallo = JSON no parseable → vacío. — `:118-144`

Linux: `Command::output()` + `serde_json`.

## 4. Waveform (seekbar)

- [ ] **generatePeaks** — siempre 400 floats (ceros si falla). — `WaveformGenerator.swift:6`, `:14-21`, `:49-52`
- [ ] **Decodificación** — `ffmpeg -i <resolved> -ac 1 -f u8 -acodec pcm_u8 -ar 200 -v quiet -y <tmp>`. — `:63-72`
- [ ] **Downsampling** — `chunk = max(1, bytes/400)`; barra = max `|b-128|/128`. — `:82-98`
- [ ] **Normalización** — divide por el máximo si >0. — `:101-104`
- [ ] **Caché** — `~/.drplayer/waveforms/<djb2hex(resolvedPath)>.wf`, 400 bytes `u8(clamp(p*255))`; tamaño incorrecto = miss. — `:8-12`, `:23-47`

## 5. Puntuación de calidad / ranking de versiones

- [ ] **score** — `drScore + formatScore` (0–100). — `AudioQualityScore.swift:21-27`
- [ ] **drScore** — `min(50, dr*4)`, 0 si nil/≤0. — `:30-34`
- [ ] **formatScore** — substring en `album.format` mayúsculas, primer match: DSF/DFF/DSD 50; FLAC/WAV/AIFF/ALAC 40; MQA 35; MP3/AAC/OGG/OPUS 10; resto 20 ("MQA FLAC" = 40). — `:38-62`
- [ ] **Badge** — `.warning` si MP3/AAC/OGG (no OPUS) o 0<DR<8; si no `.good`. — `:65-74`
- [ ] **Iconos** — `star.fill` / `checkmark.circle` / `exclamationmark.triangle`. — `:14-18`
- [ ] **rankVersions** — empatados en el máximo (si >0) → `isBest` + `.best`; orden desc. — `:77-96`

Linux: iconos Adwaita (`starred-symbolic`, `emblem-ok-symbolic`, `dialog-warning-symbolic`).

## 6. Certificación de prensaje (sidecar)

- [ ] **PressingInfo** — `discogsId`, `catalogNumber`, `label`, `country`, `year`, `formats[]`, `masteringEngineer`, `url`; v2 opcionales: `cutBy`, `pressedAt`, `recordedAt`, `mixedAt`, `matrixRunout`. — `PressingCertification.swift:7-60`
- [ ] **formatDisplay / hasDetailedInfo** — `:62-67`
- [ ] **Sidecar** — `<albumDir>/.drmobile-pressing.json`: `{version: 2, certifiedAt: ISO8601, certifiedBy: "DrPlayer", info}`; pretty + sorted keys; **compatible con DrMobile** (NFS/WebDAV). — `:74-95`
- [ ] **read/write/remove** — symlinks resueltos; errores silenciosos. — `:98-123`
- [ ] **PressingStore** — caché en memoria por ruta resuelta. — `:129-150`
- [ ] **certify(albumFullPath, discogsId)** — vía URL `drplayer://identify` (OBI Scanner): fetch Discogs → `toPressingInfo()` → persistir. — `:154-162`

Linux: handler de esquema → `.desktop` con `MimeType=x-scheme-handler/drplayer` (¿`obiplayer`?) + `GApplication::open`; `serde` con `#[serde(default)]`.

## 7. Loudness War DB (dr.loudness-war.info)

- [ ] **search(artist, album)** — `GET https://dr.loudness-war.info/album/list?artist=&album=<limpio>`, UA `DrPlayer/1.0 (music player)`, 200; `'` → `%27`. — `LoudnessWarService.swift:63-80`
- [ ] **Fallback guion** — sin resultados y artista con `-` → reintenta con `/` (AC-DC → AC/DC). — `:85-88`
- [ ] **Limpieza de título** (en orden): comillas `" " " « »`; `^\d{4}\s+`; `(...)` finales repetidos; `[...]` finales repetidos; `\s+(SHM-CD|SHM-SACD|HDCD|MQA-CD|XRCD|K2HD|HQCD|Blu-spec CD|UHQCD).*$` (ci); `\s+(Legacy|Anniversary|Collector|Limited|Special)\s+(Collection|Edition).*$`; `\s*-\s*(remaster|deluxe|bonus).*$`; `\s*-\s*(CD|Disc)\s.*$`; `\s+\d{4}\s*$`; trim. — `:23-60`
- [ ] **Scraping** — regex dotall: IDs `album/view/(\d+)`, `<tr>…</tr>`, `<td…>…</td>`; ≥8 celdas: artist, album, year, drAvg, drMin, drMax, codec, source; DR no enteros → fila descartada. — `:93-117`
- [ ] **IDs por posición** (frágil si se descartan filas). — `:137-151`
- [ ] **fetchDetail** — `/album/view/<id>`: Label, Catalog number, Country (`<th>/<td>` luego `<dt>/<dd>`). — `:157-189`
- [ ] **stripHTML** — tags + `&#039; &amp; &lt; &gt; &quot;`. — `:202-210`
- Sin caché.

Linux: `reqwest`, crate `regex` con `(?s)`.

## 8. MusicBrainz

- [ ] **Cliente** — `https://musicbrainz.org/ws/2`, UA `DrPlayer/1.0 (carlos@drplayer)`, `Accept: application/json`; ≠200 → nil sin cachear. — `MusicBrainzService.swift:55-56`, `:387-397`
- [ ] **Rate limit** — `sleep(1s)` antes de lookups por ID no cacheados; búsquedas sin throttle. — `:187`, `:329`
- [ ] **searchByCatalog** — `catno:<c> AND artist:<a>[ AND label:<l>]`, `/release/?query=…&fmt=json&limit=5`, primero. Caché `mb_search_catno_<a lc>_<catno lc>[_lbl_<l lc>]`. Sin escapar Lucene. — `:62-88`
- [ ] **searchRelease** — `release:<album> AND artist:<a>[ AND label:<l>]`, limit 25 con hints / 5 sin. Caché `mb_search_release_<a lc>_<album lc>[_<hints>]` (`fmt_X`, `cc_X`, `lbl_X`). — `:91-141`
- [ ] **Desambiguación formato/país** — primera release con `media[].format` que contenga el hint y `country` == hint; si no, `releases[0]`. — `:117-137`
- [ ] **fetchReleaseFromCache / fetchCachedRelease** — solo caché; reintenta quitando `\s*\(.*\)\s*$`. — `:144-174`
- [ ] **fetchRelease(id)** — `/release/<id>?inc=artist-credits+labels+recordings+release-groups+url-rels+artist-rels+recording-level-rels+genres&fmt=json`; caché `mb_release_<id>`. — `:176-193`
- [ ] **Parseo** — título, fecha, país, status, barcode; artista = `artist-credit[].name` con `", "`; label/catno del primer `label-info`. — `:195-216`
- [ ] **Créditos** — relaciones de release + de recording, rol = `type` o `"type (attrs)"`, dedupe (nombre, rol). — `:219-254`
- [ ] **Buckets de rol** — producers: producer/executive producer; engineers: engineer/recording/mix/balance; mastering: mastering. — `:18-40`
- [ ] **Slug Wikipedia** — `release-group.relations[].url` con `wikipedia.org` (gana el último); fallback a URL rels de la release. — `:257-277`
- [ ] **Géneros** — `release-group.genres[].name`. — `:280-284`
- [ ] **searchArtist** — `/artist/?query=artist:<enc>&fmt=json&limit=3`; caché `mb_search_artist_<name lc>`. — `:296-317`
- [ ] **fetchArtist(id)** — `?inc=url-rels+artist-rels+genres`; caché `mb_artist_<id>`; sleep 1 s. — `:319-335`
- [ ] **Parseo artista** — nombre, tipo, life-span, área, slug Wikipedia. — `:337-355`
- [ ] **Miembros** — `type=="member of band"`; rol = attrs; periodo `"begin–end"` (en dash), end → `"present"`; dedupe `name|period`. — `:358-376`

Linux: `reqwest` + `serde_json::Value`. Ojo: `.urlQueryAllowed` deja `& = + ?` sin codificar; mantener claves de caché idénticas.

## 9. Wikipedia

- [ ] **fetchSummary(slug)** — en → es. — `WikipediaService.swift:14-22`
- [ ] **searchArtist(name)** — todas las variantes en `en`, luego en `es`. — `:25-39`
- [ ] **Variantes de búsqueda** — nombre exacto; parte antes del primer separador compuesto (`" & "`, `" and "`, `" with "`, `" feat. "`, `" feat "`, `" ft. "`, `" ft "`, `" + "`, `" y "`, ci); si contiene `/`, sustituido por espacio. — `:42-66`
- [ ] **search(query, lang)** — caché `wiki_search_<lang>_<q lc>` (slug o `(none)`); slug exacto (espacios → `_`); fallback API. Fallos de red también se cachean como `(none)`. — `:69-93`
- [ ] **API búsqueda** — `https://<lang>.wikipedia.org/w/api.php?action=query&list=search&srsearch=<enc>&format=json&srlimit=1`, UA `DrPlayer/1.0`; caché `wiki_title_<lang>_<q lc>`. — `:96-122`
- [ ] **Summary** — `https://<lang>.wikipedia.org/api/rest_v1/page/summary/<slug>`; caché `wiki_<lang>_<url>` solo si válido. — `:124-153`
- [ ] **Desambiguación** — `type=="disambiguation"` o `extract` vacío → nil, siguiente fallback. — `:157-177`
- [ ] **Campos** — title, extract, thumbnail, originalimage, `content_urls.desktop.page`, idioma. — `:161-183`

Linux: codificar slug como path segment con `percent_encoding`.

## 10. Last.fm

- [ ] **API key** — `AppSettings.lastfmApiKey`; `https://ws.audioscrobbler.com/2.0`, `api_key` + `format=json`, sin UA. — `LastFMService.swift:16-20`
- [ ] **validateKey** — `artist.getinfo&artist=Radiohead`; mensajes `"Empty key"`, `"Network error"`, `"Unexpected response"` o `message` de Last.fm. — `:23-31`
- [ ] **fetchSimilarArtists(name, limit=50)** — `artist.getsimilar&autocorrect=1`; `[nombre lc: match]`; caché `lfm_similar_<name lc>_<limit>`. — `:35-69`
- [ ] **fetchAlbumTags** — `album.getinfo`; caché `lfm_album_<a lc>_<album lc>` (líneas o `(empty)`). — `:72-101`
- [ ] **fetchArtist** — `artist.getinfo`; caché `lfm_artist_<name lc>`. — `:103-124`
- [ ] **Parseo** — bio content/summary sin HTML, listeners/playcount, tags, similares, url, imagen = último de `image[]`. — `:126-176`

Linux: claves → GSettings/config o `libsecret`/`oo7`.

## 11. Discogs

- [ ] **Auth** — `key` + `secret` como query params; `isConfigured` exige ambos. — `DiscogsService.swift:46-51`
- [ ] **Cliente** — `https://api.discogs.com`, UA `DrPlayer/1.0`, 200; sin rate limit explícito. — `:331-338`
- [ ] **validateCredentials** — `/database/search?q=test&per_page=1`; 401 "Invalid key or secret", 429 "Rate limited — try again later", otros "HTTP n", "Network error". — `:54-69`
- [ ] **searchByCatalog(catno, country?, label?, format?)** — `/database/search?catno=[&label=]`; ranking; caché `discogs_catno_<catno lc>[_<country lc>][_lbl_<label lc>]` (formato fuera de la clave). — `:72-90`
- [ ] **searchByBarcode** — `?barcode=<crudo>`; caché `discogs_barcode_<bc>[_<country lc>]`. — `:93-107`
- [ ] **search(artist, album)** — `?q="artist album"&type=release`; caché `discogs_search_<a lc>_<album lc>`. — `:110-124`
- [ ] **Ranking ("prensaje exacto")** — −20 unofficial/bootleg; +15 formato preferido; si no, −5 si se prefiere SACD y no lo es; +10 país. Solo reordena, no rechaza. — `:288-329`
- [ ] **fetchRelease(id)** — `/releases/<id>`; caché `discogs_release_<id>`. — `:127-142`
- [ ] **Parseo** — título, año, país, uri, notas, label/catno de `labels[0]`, formatos `"Name (desc…)"`, géneros, estilos, imagen, tracklist, `lowest_price` `%.2f`. — `:144-197`
- [ ] **Ingenieros (extraartists)** — mastering: "mastered by/at", "remastered by"; corte: "lacquer cut by/at", "cut by", "cutting engineer"; quitar sufijo `\s*\(\d+\)$`; dedupe; `", "`. — `:200-221`
- [ ] **Compañías** — "pressed by" / "recorded at" / "mixed at" (igualdad exacta). — `:224-248`
- [ ] **Matrix/runout** — `identifiers[]` con "matrix"/"runout", `"desc: value"`, unidos con `" · "`. — `:251-261`
- [ ] **URL** — normalización a `https://www.discogs.com/...`. — `:263-270`
- [ ] **toPressingInfo** — año 0 → `""`. — `:26-42`

## 12. Letras (LRCLIB)

- [ ] **fetchLyrics** — `/get` exacto, luego `/search`. — `LyricsService.swift:20-27`
- [ ] **Exacto** — `https://lrclib.net/api/get?artist_name=&track_name=&album_name=&duration=<int>`, UA `DrPlayer/1.0 (https://github.com/drplayer)`. — `:29-49`
- [ ] **Búsqueda** — `/api/search?track_name=&artist_name=`, primer resultado sin filtrar. — `:51-70`
- [ ] **Resultado** — `plainLyrics`, `syncedLyrics`, `instrumental`. — `:72-78`
- [ ] **Parser LRC** — una marca por línea, `[mm:ss.ff]`; tags `[ar:…]` se descartan. — `:81-112`
- Sin caché.

## 13. Enriquecimiento de géneros

- [ ] **enrichGenres(album)** — devuelve solo géneros nuevos: caché (título limpio) → MB si hay tag `musicbrainzAlbumId` → Last.fm album tags (variantes de artista × título limpio/original) → fallback tags de artista. — `GenreEnricher.swift:13-76`
- [ ] **Normalización** — trim, capitalizeGenre, quitar existentes (ci), dedupe ci. — `:57-66`
- [ ] **capitalizeGenre** — palabra ≤2 chars en mayúsculas se respeta; si no, primera letra mayúscula. — `:187-195`
- [ ] **Caché negativa** — solo si hay clave Last.fm o Discogs. — `:70-73`
- [ ] **enrichAllAlbums** — secuencial, progreso `(i, total)`, salta álbumes con ≥3 géneros o en caché, 1 s entre álbumes no cacheados. — `:79-109`
- [ ] **Bug a no replicar** — el batch usa título crudo como clave y `enrichGenres` el limpio. — `:92` vs `:15`
- [ ] **cleanAlbumTitle** — comillas, año inicial, `(...)`/`[...]` finales, `\s*-\s*remaster.*$`. — `:113-140`
- [ ] **artistSearchNames** — `[name]` + `-`→`/` + `/`→`-`. — `:144-155`
- [ ] **Caché** — `~/.drplayer/genres/<djb2hex(lc("artist|album"))>.genres`; `cachedCount`. — `:6-10`, `:158-183`

## 14. Avatares de artista

- [ ] **avatar(name)** — caché → marcador `.notfound` → Wikipedia `searchArtist` (thumbnail) → Last.fm imagen → escribir `.notfound`. — `ArtistAvatarService.swift:14-45`
- [ ] **Clave** — nombre lc, espacios y `/` → `_`, máx. 80 chars (sin hash ni sanear). — `:49-54`
- [ ] **Ficheros** — `~/.drplayer/artist-avatars/<key>.jpg` (JPEG q0.8); `.notfound` permanente. — `:7-11`, `:56-77`
- [ ] **Descarga** — UA `DrPlayer/1.0 (music player; contact@drplayer.app)`, 200 + imagen válida. — `:79-85`

Linux: `gdk_pixbuf` o crate `image`; mostrar con `gdk::Texture`.
