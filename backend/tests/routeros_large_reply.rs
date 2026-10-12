use std::net::SocketAddr;
use std::time::Duration;

use rwisp_server::rwispros::{RouterClient, RouterConfig, RouterOsError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const LARGE_BURST: usize = 200;
const INCOMPLETE_BURST: usize = 3;

fn encode_word(out: &mut Vec<u8>, word: &[u8]) {
    let len = word.len();
    assert!(len < 0x80, "test words must be short");
    out.push(len as u8);
    out.extend_from_slice(word);
}

fn sentence(words: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for word in words {
        encode_word(&mut out, word);
    }
    out.push(0);
    out
}

fn decode_sentence(buf: &[u8], mut pos: usize) -> Option<(Vec<Vec<u8>>, usize)> {
    let mut words = Vec::new();
    loop {
        let first = *buf.get(pos)?;
        pos += 1;
        if first == 0 {
            return Some((words, pos));
        }
        let len = if first < 0x80 {
            first as usize
        } else {
            let second = *buf.get(pos)? as usize;
            pos += 1;
            ((first as usize & 0x7f) << 8) | second
        };
        if pos + len > buf.len() {
            return None;
        }
        words.push(buf[pos..pos + len].to_vec());
        pos += len;
    }
}

fn find_tag(words: &[Vec<u8>]) -> Option<String> {
    words
        .iter()
        .find_map(|word| word.strip_prefix(b".tag="))
        .map(|tag| String::from_utf8_lossy(tag).into_owned())
        .filter(|tag| !tag.is_empty())
}

async fn read_command_tag(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.expect("read command");
        assert!(read != 0, "client closed before sending a command");
        buf.extend_from_slice(&chunk[..read]);
        if let Some((words, _)) = decode_sentence(&buf, 0)
            && let Some(tag) = find_tag(&words)
        {
            return tag;
        }
    }
}

async fn complete_login(stream: &mut TcpStream) -> String {
    let login_tag = read_command_tag(stream).await;
    let done = sentence(&[b"!done".to_vec(), format!(".tag={login_tag}").into_bytes()]);
    stream.write_all(&done).await.expect("write login done");
    read_command_tag(stream).await
}

async fn spawn_fake_router<F>(script: F) -> (SocketAddr, tokio::task::JoinHandle<()>)
where
    F: FnOnce(String) -> Vec<u8> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake router");
    let addr = listener.local_addr().expect("fake router address");

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept client");
        let command_tag = complete_login(&mut stream).await;
        let bytes = script(command_tag);
        stream.write_all(&bytes).await.expect("write replies");
    });

    (addr, handle)
}

async fn connect(addr: SocketAddr) -> RouterClient {
    let config = RouterConfig::new(addr.ip().to_string(), "admin", "password")
        .with_port(addr.port())
        .with_timeout(Duration::from_secs(5));
    RouterClient::connect(config)
        .await
        .expect("expected to connect to the fake router")
}

#[tokio::test]
async fn large_reply_burst_is_fully_delivered() {
    let (addr, server) = spawn_fake_router(|tag| {
        let tag_word = format!(".tag={tag}").into_bytes();
        let mut bytes = Vec::new();
        for index in 0..LARGE_BURST {
            bytes.extend_from_slice(&sentence(&[
                b"!re".to_vec(),
                tag_word.clone(),
                format!("=name=user-{index:04}").into_bytes(),
                b"=profile=default".to_vec(),
            ]));
        }
        bytes.extend_from_slice(&sentence(&[b"!done".to_vec(), tag_word]));
        bytes
    })
    .await;

    let client = connect(addr).await;
    let users = client
        .get_hotspot_users()
        .await
        .expect("a large reply burst must not close the reply channel");

    assert_eq!(users.len(), LARGE_BURST);
    assert_eq!(users[0].name.as_deref(), Some("user-0000"));
    assert_eq!(users[LARGE_BURST - 1].name.as_deref(), Some("user-0199"));

    server.await.expect("fake router task");
}

#[tokio::test]
async fn incomplete_reply_burst_is_not_reported_as_success() {
    let (addr, server) = spawn_fake_router(|tag| {
        let tag_word = format!(".tag={tag}").into_bytes();
        let mut bytes = Vec::new();
        for index in 0..INCOMPLETE_BURST {
            bytes.extend_from_slice(&sentence(&[
                b"!re".to_vec(),
                tag_word.clone(),
                format!("=name=user-{index:04}").into_bytes(),
            ]));
        }
        // Intentionally omit the terminating !done: the stream closes instead.
        bytes
    })
    .await;

    let client = connect(addr).await;
    let error = client
        .get_hotspot_users()
        .await
        .expect_err("an unterminated reply must be reported as an error");

    assert!(
        matches!(error, RouterOsError::ConnectionClosed),
        "unexpected error: {error:?}"
    );

    server.await.expect("fake router task");
}
