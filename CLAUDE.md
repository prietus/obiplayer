# ObiPlayer

Port a Linux (Rust + GTK4/libadwaita) de DrPlayer (`~/drplayer`, SwiftUI + MPD), con paridad de funcionalidades. Sin mini-player por ahora.

- Plan, decisiones y bugs a no replicar: `docs/PARITY.md`. Inventario por áreas con referencias al Swift original: `docs/inventory/`.
- Conversación en español, código y comentarios en inglés.

## Estructura
- `crates/obiplayer-core` — sin GTK: cliente MPD (`mpd/`), y en adelante biblioteca, análisis y metadatos.
- `crates/obiplayer` — app GTK. Tokio corre en su runtime (`runtime()`); GTK en el hilo principal; los resultados vuelven con `on_tokio(fut).await` dentro de `glib::spawn_future_local`. Eventos de servidor vía `mpd::watch` → `async_channel`.

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
