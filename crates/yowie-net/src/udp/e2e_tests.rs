//! Loopback tests for the UDP server and client, including lossy sockets.

use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::*;
use crate::error::{ClientId, DisconnectReason, Error};
use crate::tcp::{ClientEvent, ServerEvent};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Msg {
    Num(u32),
    Blob(Vec<u8>),
}

type Server = UdpServer<Msg, Msg>;
type Client = UdpClient<Msg, Msg>;

fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for {what}");
}

fn loopback() -> UdpSocket {
    UdpSocket::bind("127.0.0.1:0").expect("bind")
}

fn lossy(cfg: LossConfig) -> LossySocket<UdpSocket> {
    LossySocket::new(loopback(), cfg)
}

fn connect(server: &Server, client: &Client) -> ClientId {
    wait("client connected", || {
        matches!(client.poll(), Some(ClientEvent::Connected)).then_some(())
    });
    match wait("server connected", || server.poll()) {
        ServerEvent::Connected { id, .. } => id,
        e => panic!("unexpected {e:?}"),
    }
}

fn recv_server(server: &Server, n: usize) -> Vec<Msg> {
    let mut out = Vec::new();
    while out.len() < n {
        if let ServerEvent::Message { message, .. } = wait("server message", || server.poll()) {
            out.push(message);
        }
    }
    out
}

fn recv_client(client: &Client, n: usize) -> Vec<Msg> {
    let mut out = Vec::new();
    while out.len() < n {
        if let ClientEvent::Message(m) = wait("client message", || client.poll()) {
            out.push(m);
        }
    }
    out
}

#[test]
fn handshake_and_messages_both_ways() {
    let server = Server::bind("127.0.0.1:0", UdpConfig::default()).expect("bind");
    let client = Client::connect(server.local_addr(), UdpConfig::default()).expect("connect");
    let id = connect(&server, &client);
    assert!(client.is_connected());
    for ch in [Channel::Reliable, Channel::Unreliable, Channel::Sequenced] {
        client.send(ch, &Msg::Num(1)).expect("send");
        assert_eq!(recv_server(&server, 1), [Msg::Num(1)], "{ch:?}");
        server.send(id, ch, &Msg::Num(2)).expect("send");
        assert_eq!(recv_client(&client, 1), [Msg::Num(2)], "{ch:?}");
    }
    server
        .broadcast(Channel::Reliable, &Msg::Num(3))
        .expect("broadcast");
    assert_eq!(recv_client(&client, 1), [Msg::Num(3)]);
    assert!(matches!(
        client.send(Channel::Unreliable, &Msg::Blob(vec![0; 2000])),
        Err(Error::TooLarge { .. })
    ));
    assert!(matches!(
        server.send(ClientId(999), Channel::Reliable, &Msg::Num(0)),
        Err(Error::UnknownClient(_))
    ));
}

#[test]
fn reliable_ordered_delivery_over_lossy_sockets() {
    let cfg = |seed| LossConfig {
        drop: 0.2,
        delay: Duration::from_millis(5),
        jitter: Duration::from_millis(12),
        seed,
    };
    let server = Server::with_socket(lossy(cfg(11)), UdpConfig::default()).expect("bind");
    let addr = server.local_addr();
    let client = Client::with_socket(lossy(cfg(22)), addr, UdpConfig::default()).expect("connect");
    let id = connect(&server, &client);

    let mut want = Vec::new();
    for i in 0..80u32 {
        let m = if i == 30 {
            Msg::Blob((0..50_000u32).map(|v| (v % 253) as u8).collect())
        } else {
            Msg::Num(i)
        };
        client.send(Channel::Reliable, &m).expect("send");
        want.push(m);
    }
    assert_eq!(recv_server(&server, want.len()), want);

    let mut back = Vec::new();
    for i in 0..40u32 {
        server
            .send(id, Channel::Reliable, &Msg::Num(i))
            .expect("send");
        back.push(Msg::Num(i));
    }
    assert_eq!(recv_client(&client, back.len()), back);
    assert!(client.stats().loss >= 0.0);
}

#[test]
fn rtt_estimate_converges_on_delayed_link() {
    let delay = LossConfig {
        delay: Duration::from_millis(20),
        ..LossConfig::default()
    };
    let server = Server::with_socket(lossy(delay.clone()), UdpConfig::default()).expect("bind");
    let client = Client::with_socket(lossy(delay), server.local_addr(), UdpConfig::default())
        .expect("connect");
    connect(&server, &client);
    for i in 0..30 {
        client.send(Channel::Reliable, &Msg::Num(i)).expect("send");
        std::thread::sleep(Duration::from_millis(25));
    }
    let rtt = wait("rtt", || {
        let r = client.stats().rtt;
        (r > Duration::ZERO).then_some(r)
    });
    // 20 ms each way, plus up to a tick of scheduling on each hop.
    let ms = rtt.as_secs_f64() * 1000.0;
    assert!((38.0..70.0).contains(&ms), "rtt {ms} ms");
}

#[test]
fn disconnects_are_reported_on_both_sides() {
    let server = Server::bind("127.0.0.1:0", UdpConfig::default()).expect("bind");
    let mut client = Client::connect(server.local_addr(), UdpConfig::default()).expect("connect");
    let id = connect(&server, &client);
    client.disconnect();
    match wait("server disconnect", || server.poll()) {
        ServerEvent::Disconnected { id: gone, reason } => {
            assert_eq!(gone, id);
            assert_eq!(reason, DisconnectReason::Remote);
        }
        e => panic!("unexpected {e:?}"),
    }

    let client2 = Client::connect(server.local_addr(), UdpConfig::default()).expect("connect");
    let id2 = connect(&server, &client2);
    server.disconnect(id2);
    wait("client sees goodbye", || {
        matches!(
            client2.poll(),
            Some(ClientEvent::Disconnected(DisconnectReason::Remote))
        )
        .then_some(())
    });
}

#[test]
fn silent_peer_times_out() {
    let cfg = UdpConfig {
        timeout: Duration::from_millis(300),
        keepalive: Duration::from_millis(100),
        ..UdpConfig::default()
    };
    let server = Server::bind("127.0.0.1:0", cfg.clone()).expect("bind");
    let client = Client::connect(server.local_addr(), cfg.clone()).expect("connect");
    connect(&server, &client);
    // Keepalives hold the link open past the timeout.
    std::thread::sleep(Duration::from_millis(700));
    assert!(client.is_connected());
    assert!(server.poll().is_none());
    drop(server);
    wait("client timeout", || match client.poll() {
        Some(ClientEvent::Disconnected(DisconnectReason::Remote | DisconnectReason::Timeout)) => {
            Some(())
        }
        _ => None,
    });
}

#[test]
fn full_server_rejects_and_dead_server_times_out() {
    let cfg = UdpConfig {
        max_clients: 1,
        ..UdpConfig::default()
    };
    let server = Server::bind("127.0.0.1:0", cfg.clone()).expect("bind");
    let first = Client::connect(server.local_addr(), cfg.clone()).expect("connect");
    connect(&server, &first);
    let second = Client::connect(server.local_addr(), cfg).expect("connect");
    wait("rejection", || {
        matches!(
            second.poll(),
            Some(ClientEvent::Disconnected(DisconnectReason::Rejected))
        )
        .then_some(())
    });

    let dead = loopback();
    let quick = UdpConfig {
        connect_timeout: Duration::from_millis(300),
        ..UdpConfig::default()
    };
    let c = Client::connect(dead.local_addr().expect("addr"), quick).expect("connect");
    wait("connect timeout", || {
        matches!(
            c.poll(),
            Some(ClientEvent::Disconnected(DisconnectReason::Timeout))
        )
        .then_some(())
    });
}

#[test]
fn waker_fires_on_traffic() {
    let server = Server::bind("127.0.0.1:0", UdpConfig::default()).expect("bind");
    let count = Arc::new(AtomicUsize::new(0));
    let c2 = count.clone();
    server.set_waker(Arc::new(move || {
        c2.fetch_add(1, Ordering::SeqCst);
    }));
    let client = Client::connect(server.local_addr(), UdpConfig::default()).expect("connect");
    connect(&server, &client);
    client.send(Channel::Reliable, &Msg::Num(1)).expect("send");
    wait("waker", || {
        (count.load(Ordering::SeqCst) >= 2).then_some(())
    });
}
