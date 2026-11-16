# ObiPlayer

Port a Linux (Rust + GTK4/libadwaita) de DrPlayer (`~/drplayer`, SwiftUI + MPD), con paridad de funcionalidades. Sin mini-player por ahora.

- Plan, decisiones y bugs a no replicar: `docs/PARITY.md`. Inventario por áreas con referencias al Swift original: `docs/inventory/`.
- Conversación en español, código y comentarios en inglés.

## Estructura
- `crates/obiplayer-core` — sin GTK: cliente MPD (`mpd/`), y en adelante biblioteca, análisis y metadatos.
- `crates/obiplayer` — app GTK con Relm4 (usar `relm4::gtk`/`relm4::adw`, no dependencias directas). Las órdenes MPD van como `sender.oneshot_command` (runtime tokio de Relm4); eventos del servidor vía `mpd::watch` → `async_channel` → `AppMsg::Mpd`. Listas grandes con `TypedGridView`/`TypedListView` (virtualizadas; la cola real tiene >5000 pistas).

## Configuración
`~/.config/obiplayer/config.toml` (se crea con valores por defecto): `[mpd] host/port/password`, `[library] music_dir` (ruta local de la biblioteca; si falta, las portadas se piden a MPD). `MPD_HOST`/`MPD_PORT` tienen prioridad.

## Pruebas
- Servidor MPD real del usuario: `192.0.2.10:6600` (sonando en un DAC; usar solo órdenes de lectura salvo que pida otra cosa).
- Probar con UNA instancia cada vez: GTK es de instancia única (una segunda se cede a la primera y sale), y lanzar muchas seguidas llegó a dejar colgado el MPD del servidor. Matar con `pkill -x obiplayer` (no `-f`: coincide con la propia orden ssh). `OBIPLAYER_ROUTE=album:<carpeta>|track:<fichero>|artist:<n>|label:<n>|composer:<n>|search:<q>|mode:<albums|artists|labels|tracks|composers>|nowplaying:<art|viz>|osc:` abre esa vista al cargar (para capturas).
- VM Linux: `ssh user@192.0.2.20` (Arch ARM, red en modo bridge, Wayland/river, `grim` para capturas; biblioteca montada en `/mnt/music` con `sudo mount -t nfs -o vers=3,ro 192.0.2.10:/mnt/music /mnt/music`, `music_dir = "/mnt/music"`). Copiar fuentes con `git ls-files -co --exclude-standard | tar -czf - -T - | ssh … 'tar -xzf - -C obiplayer'` (no hay rsync).

## Comandos
```
cargo test                                   # tests (MPD simulado, no requiere servidor)
cargo run -p obiplayer                       # app (usa MPD_HOST / MPD_PORT)
cargo run -p obiplayer-core --example mpdcli -- status|watch|queue|outputs|stats
cargo clippy --all-targets && cargo fmt
```

## Reglas
- Toda orden MPD se construye con `Command::arg` (escapa siempre). Nada de formatear strings a mano.
- Los `ACK` se propagan como `Error::Ack`; solo se ignoran explícitamente (p. ej. sticker inexistente, código 50).
- Precisión de metadatos: nunca mostrar datos que no correspondan a la edición exacta.
