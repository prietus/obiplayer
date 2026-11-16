# ObiPlayer

Port a Linux (Rust + GTK4/libadwaita) de DrPlayer (`~/drplayer`, SwiftUI + MPD), con paridad de funcionalidades. Sin mini-player por ahora.

- Plan, decisiones y bugs a no replicar: `docs/PARITY.md`. Inventario por áreas con referencias al Swift original: `docs/inventory/`.
- Conversación en español, código y comentarios en inglés.

## Estructura
- `crates/obiplayer-core` — sin GTK: cliente MPD (`mpd/`), y en adelante biblioteca, análisis y metadatos.
- `crates/obiplayer` — app GTK con Relm4 (usar `relm4::gtk`/`relm4::adw`, no dependencias directas). Las órdenes MPD van como `sender.oneshot_command` (runtime tokio de Relm4); eventos del servidor vía `mpd::watch` → `async_channel` → `AppMsg::Mpd`. Listas grandes con `TypedGridView`/`TypedListView` (virtualizadas; la cola real tiene >5000 pistas).

## Configuración
`~/.config/obiplayer/config.toml` (se crea con valores por defecto): `[mpd] host/port/password`, `[library] music_dir` (ruta local de la biblioteca; si falta, las portadas se piden a MPD), `[apis] lastfm_api_key/discogs_key/discogs_secret` (el fichero se guarda con permisos 600; nunca imprimir ni registrar las claves). `MPD_HOST`/`MPD_PORT` tienen prioridad.

## Pruebas
- Entorno de pruebas (servidor MPD real del usuario y VM Linux): ver `CLAUDE.local.md` (no versionado). El servidor real está sonando en un DAC: solo órdenes de lectura salvo que el usuario pida otra cosa.
- Probar con UNA instancia cada vez: GTK es de instancia única (una segunda se cede a la primera y sale), y lanzar muchas seguidas llegó a dejar colgado el MPD del servidor. El usuario puede tener ObiPlayer instalado y abierto en la VM: NO usar `pkill -x obiplayer`; lanzar la instancia de prueba en segundo plano, guardar su PID (`$!`) y matar solo ese PID. `OBIPLAYER_ROUTE=album:<carpeta>|track:<fichero>|artist:<n>|label:<n>|composer:<n>|search:<q>|mode:<albums|artists|labels|tracks|composers>|nowplaying:<art|viz|lyrics>|osc:` abre esa vista al cargar (para capturas). En pruebas usar siempre `OBIPLAYER_NO_COUNT=1` (no contar reproducciones en los stickers del servidor real).
- No usar `screencapture` en el Mac (pide permiso de Grabación de pantalla y captura el escritorio del usuario): las capturas, en la VM.
- VM Linux de pruebas: Arch ARM, Wayland/river, `grim` para capturas; biblioteca montada por NFS v3 en `/mnt/music`. Copiar fuentes con `git ls-files -co --exclude-standard | tar -czf - -T - | ssh … 'tar -xzf - -C obiplayer'` (no hay rsync).

## Comandos
```
cargo test                                   # tests (MPD simulado, no requiere servidor)
cargo run -p obiplayer                       # app (usa MPD_HOST / MPD_PORT)
cargo run -p obiplayer-core --example mpdcli -- status|watch|queue|outputs|stats
cargo clippy --all-targets && cargo fmt
```

## Traducciones y empaquetado
- Las cadenas de la UI van en inglés dentro de `tr("…")` (o de los helpers `icon_button`, `InfoGrid::text`…). Tras añadir cadenas: `python3 scripts/extract_strings.py` y traducirlas en `po/es.po` (se embebe al compilar).
- Paquete Arch: `packaging/arch/PKGBUILD` (probado en la VM). Flatpak: `packaging/flatpak/`.

## Reglas
- Toda orden MPD se construye con `Command::arg` (escapa siempre). Nada de formatear strings a mano.
- Los `ACK` se propagan como `Error::Ack`; solo se ignoran explícitamente (p. ej. sticker inexistente, código 50).
- Precisión de metadatos: nunca mostrar datos que no correspondan a la edición exacta.
