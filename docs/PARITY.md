# ObiPlayer — paridad con DrPlayer

Port de DrPlayer (macOS, SwiftUI, ~18.8k líneas) a Linux con Rust + GTK4/libadwaita.
**Fuera de alcance por ahora:** mini-player.

Inventario detallado (checklists con referencias `Fichero.swift:línea` al original en `~/drplayer/DrPlayer/Sources/`):

| Fichero | Contenido |
|---|---|
| [inventory/01-core.md](inventory/01-core.md) | Cliente MPD, polling, cola, biblioteca y álbumes, agregaciones, versiones, ratings/historial, ajustes y mpd.conf, carpetas remotas, portadas, cachés, smart playlists, radio, sample-rate/DAC, Now Playing, shell, temas |
| [inventory/02-analysis-metadata.md](inventory/02-analysis-metadata.md) | DR14, muestras del visualizador, ffprobe, waveform, puntuación de calidad, certificación de prensaje, Loudness War, MusicBrainz, Wikipedia, Last.fm, Discogs, LRCLIB, géneros, avatares |
| [inventory/03-main-views.md](inventory/03-main-views.md) | Ventana raíz, barra de reproducción, Home, grid, detalle de álbum, info de álbum, detalle de pista, lista de pistas, TrackRow, cola, búsqueda, pantalla completa, letras, waveform, visualizadores |
| [inventory/04-secondary-views.md](inventory/04-secondary-views.md) | Artistas, compositores, productores, sellos, estadísticas, comparación DR y de ediciones, smart playlists, signal path, tareas en segundo plano, ajustes, asistente inicial, carpetas de biblioteca, localización |

---

## Correspondencia de plataforma

| macOS | Linux |
|---|---|
| SwiftUI + `@Observable` | gtk4-rs + libadwaita (+ Relm4) |
| Network.framework | tokio `TcpStream` |
| `posix_spawn` ffmpeg/ffprobe | `std::process::Command` (stdout directo, sin ficheros temporales) |
| Accelerate/vDSP | Rust puro / `realfft` |
| URLSession | `reqwest` |
| UserDefaults | TOML en `$XDG_CONFIG_HOME/obiplayer/` (secretos en libsecret) |
| MediaPlayer (Now Playing) | MPRIS2 (`mpris-server`/zbus) |
| CoreAudio (dispositivos, rate switching, agregados) | ALSA `hw:` / PipeWire; sin agregados |
| `getmntinfo` + `/Volumes` | `/proc/self/mounts` + `/media`, `/run/media`, `/mnt`, gvfs |
| NSOpenPanel / NSWorkspace | `gtk::FileDialog` / `UriLauncher` / `FileLauncher` |
| `brew services` | `systemctl --user` |
| Esquema `drplayer://` | `.desktop` con `x-scheme-handler` + `GApplication::open` |
| Localizable.strings | gettext |

## Decisiones (2026-11-16)

1. **Caché**: `$XDG_CACHE_HOME/obiplayer/` (normalmente `~/.cache/obiplayer`). Mismas claves djb2 y formatos que DrPlayer, para poder copiar cachés.
2. **Integraciones Mac-only** (drtagger, Dr. Doctor, OBIScanner/ArtworkServer :18923, esquema `drplayer://identify`): **fuera de alcance** por ahora. El sidecar `.drmobile-pressing.json` sí se lee/escribe con formato idéntico.
3. **Polling → `idle`**: conexión de comandos persistente + conexión `idle` dedicada (hecho en fase 1).
4. **Bitperfect**: regla endurecida en Linux (además de `mixer_type none`: `replaygain off`, sin resampling/conversión — `auto_resample`/`auto_format` no — y salida a dispositivo `hw:`). Detalle al implementar signal path.
5. **Sample-rate matching**: con ALSA `hw:` es implícito; para PipeWire se evaluará en la fase de análisis.
6. **Smart playlists**: pendiente decidir si se importa el JSON de DrPlayer.

## Bugs del original que no conviene replicar

- Argumentos MPD sin escapar (`search`, URIs) — solo `LibraryFolders` escapa.
- Password MPD incorrecta falla en silencio (ACK ignorado); "Test connection" no envía password.
- Parser de mpd.conf por prefijo (`port` casa con claves más largas).
- Historial TSV sin escapar tabs.
- `GenreEnricher.enrichAllAlbums` usa título crudo como clave de caché y `enrichGenres` el limpio.
- Wikipedia cachea fallos de red como `(none)` (30 días).
- Loudness War asigna IDs por posición.
- DR14 sin invalidación por mtime/tamaño.
- Artistas/Sellos: al elegir un álbum del detalle, el padre recibe el artista/sello.
- TrackListView: "Add to queue" desde detalle reproduce.
- Smart playlists: "+ Add Rule" no reevalúa.
- MPRIS: Play/Pause deben ser comandos reales, no toggle.
- Localización: la mayoría de cadenas no traducibles; usar gettext desde el inicio.

## Lógica duplicada a centralizar en `core`

- Colores DR/formato y barra DR (6+ vistas).
- Normalización de títulos (al menos 4 variantes: cover, géneros, Loudness War, ediciones) — mantener cada variante donde afecte a claves de caché o resultados, pero en un único módulo.
- Cascada de búsqueda de release MB/Discogs (AlbumDetail, AlbumInfo, EditionComparison) → un servicio `ReleaseInfo` con resultado compartido.
- Matching de versiones/ediciones.

## Fases propuestas

1. ~~**core: MPD**~~ ✅ — cliente, parsers, stickers, outputs, `idle`; tests con servidor MPD simulado.
2. ~~**App mínima**~~ ✅ — ventana (Relm4), barra de reproducción, transporte, cola, grid de álbumes; portadas locales o vía MPD (`albumart`/`readpicture`). Temporal: clic en álbum = reproducir (el detalle llega en la fase 3).
3. **Biblioteca** — detalle de álbum, pistas, ratings, versiones, búsqueda, modos de navegación.
4. **Análisis** — DR14, waveform, ffprobe, visualizadores (FIFO), signal path.
5. **Metadatos** — MB, Discogs, Wikipedia, Last.fm, LRCLIB, Loudness War, géneros, avatares, portadas online, cachés.
6. **Funciones avanzadas** — ediciones, productores, smart playlists, radio, estadísticas, Home.
7. **Sistema** — ajustes, asistente, carpetas remotas, MPRIS, temas, gettext, empaquetado (Flatpak/.deb).
