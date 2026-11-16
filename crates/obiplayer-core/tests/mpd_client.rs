//! Client behaviour against a scripted fake MPD server.

use std::time::Duration;

use obiplayer_core::mpd::{Ack, Client, ConnectConfig, Error, Event, PlayState, Subsystem, watch};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// One accepted connection: a list of (expected request lines, raw response).
type Script = Vec<(&'static str, &'static str)>;

/// Serve one script per accepted connection, in order. Each connection is
/// closed when its script ends (like MPD's connection_timeout would).
async fn fake_mpd(scripts: Vec<Script>) -> (ConnectConfig, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        for script in scripts {
            let (stream, _) = listener.accept().await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut r = BufReader::new(r);
            w.write_all(b"OK MPD 0.24.0\n").await.unwrap();
            for (expected, response) in script {
                for want in expected.lines() {
                    let mut line = String::new();
                    r.read_line(&mut line).await.unwrap();
                    // Every connection negotiates a binary limit; accept it.
                    while line.starts_with("binarylimit ") {
                        w.write_all(b"OK\n").await.unwrap();
                        line.clear();
                        r.read_line(&mut line).await.unwrap();
                    }
                    assert_eq!(line.trim_end_matches('\n'), want);
                }
                w.write_all(response.as_bytes()).await.unwrap();
            }
        }
    });
    let mut cfg = ConnectConfig::new("127.0.0.1", port);
    cfg.timeout = Duration::from_secs(5);
    (cfg, handle)
}

#[tokio::test]
async fn status_and_quoted_arguments() {
    let (cfg, server) = fake_mpd(vec![vec![
        ("status", "state: play\nsong: 2\naudio: 44100:16:2\nOK\n"),
        (r#"add "AC\\DC/Back in \"Black\"""#, "OK\n"),
    ]])
    .await;
    let c = Client::new(cfg);
    let st = c.status().await.unwrap();
    assert_eq!(st.state, PlayState::Play);
    assert_eq!(st.song_pos, Some(2));
    c.add(r#"AC\DC/Back in "Black""#).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_reported() {
    let (cfg, server) = fake_mpd(vec![vec![(
        r#"password "nope""#,
        "ACK [3@0] {password} incorrect password\n",
    )]])
    .await;
    let err = Client::connect(cfg.with_password("nope"))
        .await
        .err()
        .unwrap();
    assert_eq!(err.ack_code(), Some(Ack::PASSWORD));
    server.await.unwrap();
}

#[tokio::test]
async fn reconnects_after_server_drops_connection() {
    let (cfg, server) = fake_mpd(vec![
        vec![("status", "state: stop\nOK\n")],
        vec![("status", "state: pause\nOK\n")],
    ])
    .await;
    let c = Client::new(cfg);
    assert_eq!(c.status().await.unwrap().state, PlayState::Stop);
    // First connection is now closed by the server; this must reconnect.
    assert_eq!(c.status().await.unwrap().state, PlayState::Pause);
    server.await.unwrap();
}

#[tokio::test]
async fn ack_errors_are_not_swallowed() {
    let (cfg, server) = fake_mpd(vec![vec![(
        r#"add "missing.flac""#,
        "ACK [50@0] {add} No such directory\n",
    )]])
    .await;
    let err = Client::new(cfg).add("missing.flac").await.unwrap_err();
    assert!(matches!(err, Error::Ack(ref a) if a.code == Ack::NO_EXIST && a.command == "add"));
    server.await.unwrap();
}

#[tokio::test]
async fn stickers() {
    let (cfg, server) = fake_mpd(vec![vec![
        (
            r#"sticker "get" "song" "a.flac" "rating""#,
            "ACK [50@0] {sticker} no such sticker\n",
        ),
        (
            r#"sticker "get" "song" "b.flac" "play_count""#,
            "sticker: play_count=4\nOK\n",
        ),
        (r#"sticker "set" "song" "b.flac" "play_count" "5""#, "OK\n"),
        (
            r#"sticker "find" "song" "" "preferred""#,
            "file: x/1.flac\nsticker: preferred=1\nfile: y/2.dsf\nsticker: preferred=1\nOK\n",
        ),
        (
            r#"sticker "delete" "song" "c.flac" "preferred""#,
            "ACK [50@0] {sticker} no such sticker\n",
        ),
    ]])
    .await;
    let c = Client::new(cfg);
    assert_eq!(c.sticker_get("a.flac", "rating").await.unwrap(), None);
    assert_eq!(
        c.sticker_increment("b.flac", "play_count", 1)
            .await
            .unwrap(),
        5
    );
    let found = c.sticker_find("", "preferred").await.unwrap();
    assert_eq!(
        found,
        vec![
            ("x/1.flac".into(), "1".into()),
            ("y/2.dsf".into(), "1".into())
        ]
    );
    c.sticker_delete("c.flac", "preferred").await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn replace_queue_uses_one_command_list() {
    let (cfg, server) = fake_mpd(vec![vec![(
        "command_list_begin\nclear\nadd \"a.flac\"\nadd \"b.flac\"\nplay\ncommand_list_end",
        "OK\n",
    )]])
    .await;
    Client::new(cfg)
        .replace_queue_and_play(&["a.flac".into(), "b.flac".into()])
        .await
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn add_id_and_update_return_ids() {
    let (cfg, server) = fake_mpd(vec![vec![
        (r#"addid "x.flac""#, "Id: 17\nOK\n"),
        (r#"update "Rock""#, "updating_db: 3\nOK\n"),
    ]])
    .await;
    let c = Client::new(cfg);
    assert_eq!(c.add_id("x.flac").await.unwrap(), 17);
    assert_eq!(c.update(Some("Rock")).await.unwrap(), 3);
    server.await.unwrap();
}

#[tokio::test]
async fn watch_reports_changes_and_reconnects() {
    let (cfg, server) = fake_mpd(vec![
        vec![("idle", "changed: player\nchanged: mixer\nOK\n")],
        vec![("idle", "changed: database\nOK\n")],
    ])
    .await;
    let (tx, rx) = async_channel::unbounded();
    let task = tokio::spawn(watch(cfg, tx));

    let next = || async {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    };
    assert!(matches!(next().await, Event::Connected { version } if version == "0.24.0"));
    assert_eq!(
        next().await,
        Event::Changed(vec![Subsystem::Player, Subsystem::Mixer])
    );
    assert!(matches!(next().await, Event::Disconnected { .. }));
    assert!(matches!(next().await, Event::Connected { .. }));
    assert_eq!(next().await, Event::Changed(vec![Subsystem::Database]));

    drop(rx);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn cover_art_is_read_in_chunks_with_fallback() {
    let (cfg, server) = fake_mpd(vec![vec![
        // No cover file for the first song, embedded picture instead.
        (
            r#"albumart "a/1.flac" "0""#,
            "ACK [50@0] {albumart} No file exists\n",
        ),
        (
            r#"readpicture "a/1.flac" "0""#,
            "size: 6\ntype: image/png\nbinary: 4\nabcd\nOK\n",
        ),
        (
            r#"readpicture "a/1.flac" "4""#,
            "size: 6\ntype: image/png\nbinary: 2\nef\nOK\n",
        ),
        // Nothing at all for the second.
        (
            r#"albumart "b/2.flac" "0""#,
            "ACK [50@0] {albumart} No file exists\n",
        ),
        (r#"readpicture "b/2.flac" "0""#, "OK\n"),
    ]])
    .await;
    let c = Client::new(cfg);
    assert_eq!(
        c.cover_art("a/1.flac").await.unwrap().as_deref(),
        Some(&b"abcdef"[..])
    );
    assert_eq!(c.cover_art("b/2.flac").await.unwrap(), None);
    server.await.unwrap();
}
