//! Loopback tests for the TCP server and client, with and without TLS.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::*;
use crate::error::{ClientId, DisconnectReason};
use crate::tls::{TlsClient, TlsServer};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Msg {
    Hello(String),
    Number(u64),
    Blob(Vec<u8>),
}

type Server = TcpServer<Msg, Msg>;
type Client = TcpClient<Msg, Msg>;

fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for {what}");
}

fn next_server_event(s: &Server) -> ServerEvent<Msg> {
    wait("server event", || s.poll())
}

fn next_client_event(c: &Client) -> ClientEvent<Msg> {
    wait("client event", || c.poll())
}

/// Connects a client, returning it and the server-side id.
fn pair(server: &Server, client: &Client) -> ClientId {
    assert!(matches!(next_client_event(client), ClientEvent::Connected));
    match next_server_event(server) {
        ServerEvent::Connected { id, addr } => {
            assert!(addr.ip().is_loopback());
            id
        }
        e => panic!("unexpected {e:?}"),
    }
}

fn round_trip(server: Server, client: Client) {
    let id = pair(&server, &client);
    client.send(&Msg::Hello("hi".into())).expect("send");
    client.send(&Msg::Number(7)).expect("send");
    let big = Msg::Blob(vec![0xab; 300_000]);
    client.send(&big).expect("send");
    let mut got = Vec::new();
    while got.len() < 3 {
        if let ServerEvent::Message { id: from, message } = next_server_event(&server) {
            assert_eq!(from, id);
            got.push(message);
        }
    }
    assert_eq!(got, [Msg::Hello("hi".into()), Msg::Number(7), big.clone()]);
    server.send(id, &Msg::Number(99)).expect("reply");
    server.broadcast(&big).expect("broadcast");
    for want in [Msg::Number(99), big] {
        match next_client_event(&client) {
            ClientEvent::Message(m) => assert_eq!(m, want),
            e => panic!("unexpected {e:?}"),
        }
    }
    assert_eq!(server.clients(), vec![id]);
    assert!(server.client_addr(id).is_some());
}

#[test]
fn plain_round_trip() {
    let server = Server::listen("127.0.0.1:0", TcpConfig::default()).expect("listen");
    let client = Client::connect(server.local_addr(), TcpConfig::default()).expect("connect");
    round_trip(server, client);
}

fn self_signed() -> (String, String) {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert");
    (ck.cert.pem(), ck.signing_key.serialize_pem())
}

#[test]
fn tls_round_trip_with_trusted_and_pinned_certificate() {
    let (cert, key) = self_signed();
    for pinned in [false, true] {
        let tls = TlsServer::from_pem(cert.as_bytes(), key.as_bytes()).expect("server tls");
        let server = Server::listen_tls("127.0.0.1:0", TcpConfig::default(), tls).expect("listen");
        let ctls = if pinned {
            TlsClient::pinned_pem("localhost", cert.as_bytes())
        } else {
            TlsClient::with_roots_pem("localhost", cert.as_bytes())
        }
        .expect("client tls");
        let client =
            Client::connect_tls(server.local_addr(), TcpConfig::default(), ctls).expect("connect");
        round_trip(server, client);
    }
}

#[test]
fn tls_rejects_untrusted_and_wrongly_pinned_certificates() {
    let (cert, key) = self_signed();
    let (other, _) = self_signed();
    let tls = TlsServer::from_pem(cert.as_bytes(), key.as_bytes()).expect("server tls");
    let server = Server::listen_tls("127.0.0.1:0", TcpConfig::default(), tls).expect("listen");
    let cases = [
        TlsClient::with_roots_pem("localhost", other.as_bytes()),
        TlsClient::pinned_pem("localhost", other.as_bytes()),
        TlsClient::with_roots_pem("wrong.example", cert.as_bytes()),
    ];
    for ctls in cases {
        let client = Client::connect_tls(
            server.local_addr(),
            TcpConfig::default(),
            ctls.expect("client tls"),
        )
        .expect("starts");
        match next_client_event(&client) {
            ClientEvent::Disconnected(DisconnectReason::Io(_)) => {}
            e => panic!("unexpected {e:?}"),
        }
        assert_eq!(client.state(), ConnState::Closed);
        assert!(client.send(&Msg::Number(1)).is_err());
    }
    assert!(server.poll().is_none());
}

#[test]
fn disconnect_and_oversize_events() {
    let cfg = TcpConfig {
        max_message: 1000,
        ..TcpConfig::default()
    };
    let server = Server::listen("127.0.0.1:0", cfg.clone()).expect("listen");
    let mut client = Client::connect(server.local_addr(), cfg.clone()).expect("connect");
    let id = pair(&server, &client);
    assert!(client.send(&Msg::Blob(vec![0; 5000])).is_err());
    client.send(&Msg::Number(1)).expect("send");
    client.disconnect();
    assert!(matches!(
        next_server_event(&server),
        ServerEvent::Message { .. }
    ));
    match next_server_event(&server) {
        ServerEvent::Disconnected { id: gone, reason } => {
            assert_eq!(gone, id);
            assert_eq!(reason, DisconnectReason::Remote);
        }
        e => panic!("unexpected {e:?}"),
    }

    // A peer that sends an oversize frame is dropped.
    let big = Client::connect(
        server.local_addr(),
        TcpConfig {
            max_message: 100_000,
            ..cfg
        },
    )
    .expect("connect");
    let id = pair(&server, &big);
    big.send(&Msg::Blob(vec![1; 5000]))
        .expect("fits client limit");
    match next_server_event(&server) {
        ServerEvent::Disconnected {
            id: gone,
            reason: DisconnectReason::Protocol(_),
        } => assert_eq!(gone, id),
        e => panic!("unexpected {e:?}"),
    }
}

#[test]
fn server_disconnect_and_shutdown() {
    let mut server = Server::listen("127.0.0.1:0", TcpConfig::default()).expect("listen");
    let client = Client::connect(server.local_addr(), TcpConfig::default()).expect("connect");
    let id = pair(&server, &client);
    server.send(id, &Msg::Number(5)).expect("send");
    server.disconnect(id);
    assert!(matches!(
        next_client_event(&client),
        ClientEvent::Message(Msg::Number(5))
    ));
    assert!(matches!(
        next_client_event(&client),
        ClientEvent::Disconnected(DisconnectReason::Remote)
    ));
    assert!(matches!(
        next_server_event(&server),
        ServerEvent::Disconnected { .. }
    ));
    assert!(server.send(id, &Msg::Number(1)).is_err());
    server.shutdown();
}

#[test]
fn connect_failure_and_cancellation() {
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr")
    };
    let c = Client::connect(dead, TcpConfig::default()).expect("starts");
    assert!(matches!(
        next_client_event(&c),
        ClientEvent::Disconnected(DisconnectReason::Io(_))
    ));
    let server = Server::listen("127.0.0.1:0", TcpConfig::default()).expect("listen");
    let mut c = Client::connect(server.local_addr(), TcpConfig::default()).expect("connect");
    c.disconnect();
    assert_eq!(c.state(), ConnState::Closed);
}

#[test]
fn waker_fires_when_traffic_arrives() {
    let server = Server::listen("127.0.0.1:0", TcpConfig::default()).expect("listen");
    let count = Arc::new(AtomicUsize::new(0));
    let c2 = count.clone();
    server.set_waker(Arc::new(move || {
        c2.fetch_add(1, Ordering::SeqCst);
    }));
    let client = Client::connect(server.local_addr(), TcpConfig::default()).expect("connect");
    pair(&server, &client);
    client.send(&Msg::Number(1)).expect("send");
    wait("waker", || {
        (count.load(Ordering::SeqCst) >= 2).then_some(())
    });
}
