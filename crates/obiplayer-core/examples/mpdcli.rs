//! Tiny CLI to exercise the client against a real server.
//! Honors MPD_HOST / MPD_PORT.
//!
//!   cargo run -p obiplayer-core --example mpdcli -- status
//!   cargo run -p obiplayer-core --example mpdcli -- watch

use obiplayer_core::mpd::{Client, ConnectConfig, watch};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = ConnectConfig::from_env();
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "status".into());
    match cmd.as_str() {
        "watch" => {
            let (tx, rx) = async_channel::unbounded();
            tokio::spawn(watch(cfg, tx));
            while let Ok(ev) = rx.recv().await {
                println!("{ev:?}");
            }
        }
        "stats" => println!("{:#?}", Client::connect(cfg).await?.stats().await?),
        "outputs" => println!("{:#?}", Client::connect(cfg).await?.outputs().await?),
        "queue" => {
            for s in Client::connect(cfg).await?.queue().await? {
                println!(
                    "{:>4} {} — {}",
                    s.pos.unwrap_or(0),
                    s.artist().unwrap_or("?"),
                    s.display_title()
                );
            }
        }
        _ => {
            let c = Client::connect(cfg).await?;
            let st = c.status().await?;
            let song = c.current_song().await?;
            println!("{:#?}", st);
            if let Some(s) = song {
                println!(
                    "{} — {} [{}]",
                    s.artist().unwrap_or("?"),
                    s.display_title(),
                    s.album().unwrap_or("?")
                );
            }
            if let Some(a) = st.audio {
                println!("audio: {a}");
            }
        }
    }
    Ok(())
}
