//! A chat room over `yowie-net`. Run one copy with `--listen :7777` and
//! others with `--join host:7777`. Typed lines travel over TCP, and every
//! player's pointer position is shared over UDP. With `--reliable` the chat
//! lines go over the reliable UDP channel instead, and a panel shows how
//! many arrived in order and the link's round trip and loss.
//!
//! Both ends run the turn-based loop and sleep until input or traffic: the
//! network threads call the context's `Waker` when a message is ready.
//! `--name <n>` sets your name; `--bot` sends a greeting (20 numbered lines
//! with `--reliable`) once connected, for trying it without a second
//! keyboard.

#[path = "../common/mod.rs"]
mod common;
mod wire;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use wire::{Down, Up, Wire, host_of, listen_address};
use yowie::gfx::Font;
use yowie::input::Key;
use yowie::net::{
    Channel, ClientEvent, ClientId, ServerEvent, TcpClient, TcpConfig, TcpServer, UdpClient,
    UdpConfig, UdpServer,
};
use yowie::{Color, Config, Context, Game, GameResult, LoopMode, Rect, Vec2, Waker, vec2};

enum Role {
    Host {
        tcp: TcpServer<Up, Down>,
        udp: UdpServer<Wire, Wire>,
        names: HashMap<ClientId, String>,
    },
    Guest {
        tcp: TcpClient<Down, Up>,
        udp: Option<UdpClient<Wire, Wire>>,
        addr: String,
    },
    Alone,
}

struct Chat {
    font: Font,
    role: Role,
    name: String,
    bot: bool,
    reliable: bool,
    lines: Vec<String>,
    draft: String,
    points: HashMap<String, Vec2>,
    mouse: Vec2,
    sent: u32,
    received: u32,
    last_seq: HashMap<String, u32>,
    in_order: bool,
}

fn arg(name: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args.next();
        }
    }
    None
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

/// A callback that wakes the sleeping loop from a network thread.
fn waker_fn(w: &Waker) -> Arc<dyn Fn() + Send + Sync> {
    let w = w.clone();
    Arc::new(move || w.wake())
}

impl Chat {
    fn say(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
        if self.lines.len() > 14 {
            self.lines.remove(0);
        }
    }

    /// Delivers a line to everyone else: over TCP, or with `--reliable`
    /// over the reliable UDP channel.
    fn send_chat(&mut self, text: String) {
        let from = self.name.clone();
        let seq = self.sent;
        let result = match (&self.role, self.reliable) {
            (Role::Alone, _) => Ok(()),
            (Role::Guest { tcp, .. }, false) => tcp.send(&Up::Chat { from, text }),
            (Role::Host { tcp, .. }, false) => {
                tcp.broadcast(&Down::Chat { from, text }).map(|_| ())
            }
            (Role::Guest { udp: Some(udp), .. }, true) => {
                udp.send(Channel::Reliable, &Wire::Chat { from, text, seq })
            }
            (Role::Guest { udp: None, .. }, true) => Ok(()),
            (Role::Host { udp, .. }, true) => {
                udp.broadcast(Channel::Reliable, &Wire::Chat { from, text, seq })
            }
        };
        self.sent += 1;
        if let Err(e) = result {
            self.say(format!("send failed: {e}"));
        }
    }

    fn receive_reliable(&mut self, from: &str, text: &str, seq: u32) {
        self.received += 1;
        if self.last_seq.get(from).is_some_and(|&last| seq != last + 1) {
            self.in_order = false;
        }
        self.last_seq.insert(from.to_owned(), seq);
        self.say(format!("{from}: {text}"));
    }

    fn poll_host(&mut self) {
        let Role::Host { tcp, udp, names } = &mut self.role else {
            return;
        };
        let mut said = Vec::new();
        while let Some(ev) = tcp.poll() {
            match ev {
                ServerEvent::Connected { id, addr } => {
                    let _ = tcp.send(
                        id,
                        &Down::Welcome {
                            udp_port: udp.local_addr().port(),
                        },
                    );
                    said.push(format!("Peer {id} connected from {addr}"));
                }
                ServerEvent::Disconnected { id, reason } => {
                    let name = names.remove(&id).unwrap_or_else(|| id.to_string());
                    said.push(format!("{name} left ({reason:?})"));
                }
                ServerEvent::Message { id, message } => match message {
                    Up::Join(name) => {
                        said.push(format!("{name} joined"));
                        let hello = Down::Chat {
                            from: "host".into(),
                            text: format!("{name} joined"),
                        };
                        let _ = tcp.broadcast(&hello);
                        names.insert(id, name);
                    }
                    Up::Chat { from, text } => {
                        said.push(format!("{from}: {text}"));
                        let _ = tcp.broadcast_except(&Down::Chat { from, text }, Some(id));
                    }
                },
            }
        }
        let mut reliable = Vec::new();
        while let Some(ev) = udp.poll() {
            if let ServerEvent::Message { id, message } = ev {
                match &message {
                    Wire::Pointer { name, x, y } => {
                        self.points.insert(name.clone(), vec2(*x, *y));
                        let _ = udp.broadcast_except(Channel::Sequenced, &message, id);
                    }
                    Wire::Chat { from, text, seq } => {
                        reliable.push((from.clone(), text.clone(), *seq));
                        let _ = udp.broadcast_except(Channel::Reliable, &message, id);
                    }
                }
            }
        }
        for line in said {
            self.say(line);
        }
        for (from, text, seq) in reliable {
            self.receive_reliable(&from, &text, seq);
        }
    }

    fn poll_guest(&mut self, ctx: &Context) {
        let Role::Guest { tcp, udp, addr } = &mut self.role else {
            return;
        };
        let (mut said, mut reliable, mut greet) = (Vec::new(), Vec::new(), false);
        while let Some(ev) = tcp.poll() {
            match ev {
                ClientEvent::Connected => said.push(format!("Connected to {addr}")),
                ClientEvent::Disconnected(reason) => {
                    said.push(format!("Disconnected ({reason:?})"))
                }
                ClientEvent::Message(Down::Welcome { udp_port }) => {
                    match UdpClient::connect((host_of(addr), udp_port), UdpConfig::default()) {
                        Ok(client) => {
                            client.set_waker(waker_fn(&ctx.waker()));
                            *udp = Some(client);
                            greet = true;
                        }
                        Err(e) => said.push(format!("UDP link failed: {e}")),
                    }
                }
                ClientEvent::Message(Down::Chat { from, text }) => {
                    said.push(format!("{from}: {text}"))
                }
            }
        }
        if let Some(client) = udp {
            while let Some(ev) = client.poll() {
                match ev {
                    ClientEvent::Message(Wire::Pointer { name, x, y }) => {
                        self.points.insert(name, vec2(x, y));
                    }
                    ClientEvent::Message(Wire::Chat { from, text, seq }) => {
                        reliable.push((from, text, seq));
                    }
                    ClientEvent::Connected => said.push("UDP link up".into()),
                    ClientEvent::Disconnected(reason) => {
                        said.push(format!("UDP link down ({reason:?})"))
                    }
                }
            }
        }
        for line in said {
            self.say(line);
        }
        for (from, text, seq) in reliable {
            self.receive_reliable(&from, &text, seq);
        }
        if greet && self.bot {
            let lines = if self.reliable { 20 } else { 1 };
            for i in 0..lines {
                let text = if self.reliable {
                    format!("reliable line {}", i + 1)
                } else {
                    "hello from a bot".to_owned()
                };
                self.send_chat(text);
            }
        }
    }

    fn share_pointer(&mut self, mouse: Vec2) {
        if mouse == self.mouse {
            return;
        }
        self.mouse = mouse;
        self.points.insert(self.name.clone(), mouse);
        let msg = Wire::Pointer {
            name: self.name.clone(),
            x: mouse.x,
            y: mouse.y,
        };
        match &self.role {
            Role::Host { udp, .. } => {
                let _ = udp.broadcast(Channel::Sequenced, &msg);
            }
            Role::Guest { udp: Some(udp), .. } => {
                let _ = udp.send(Channel::Sequenced, &msg);
            }
            _ => {}
        }
    }

    /// The round trip and loss of the first UDP link, for the status panel.
    fn link_stats(&self) -> Option<(Duration, f32)> {
        match &self.role {
            Role::Host { udp, .. } => udp.clients().first().and_then(|&id| udp.stats(id)),
            Role::Guest { udp: Some(udp), .. } => Some(udp.stats()),
            _ => None,
        }
        .map(|s| (s.rtt, s.loss))
    }
}

impl Game for Chat {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let waker = waker_fn(&ctx.waker());
        let name = arg("--name").unwrap_or_else(|| "player".into());
        let mut lines = Vec::new();
        let role = match (arg("--listen"), arg("--join")) {
            (Some(listen), _) => {
                let addr = listen_address(&listen);
                let tcp = TcpServer::listen(&addr, TcpConfig::default())?;
                let udp = UdpServer::bind("0.0.0.0:0", UdpConfig::default())?;
                tcp.set_waker(waker.clone());
                udp.set_waker(waker);
                lines.push(format!(
                    "Listening on {} (UDP {}). Others join with --join {}",
                    tcp.local_addr(),
                    udp.local_addr().port(),
                    tcp.local_addr()
                ));
                Role::Host {
                    tcp,
                    udp,
                    names: HashMap::new(),
                }
            }
            (None, Some(addr)) => {
                let tcp = TcpClient::connect(&addr, TcpConfig::default())?;
                tcp.set_waker(waker);
                tcp.send(&Up::Join(name.clone()))?;
                Role::Guest {
                    tcp,
                    udp: None,
                    addr,
                }
            }
            (None, None) => {
                lines.push(
                    "Run with --listen :7777 to host, or --join host:7777 to connect.".into(),
                );
                Role::Alone
            }
        };
        Ok(Chat {
            font: ctx.gfx.default_font(),
            role,
            name,
            bot: flag("--bot"),
            reliable: flag("--reliable"),
            lines,
            draft: String::new(),
            points: HashMap::new(),
            mouse: Vec2::NEG_ONE,
            sent: 0,
            received: 0,
            last_seq: HashMap::new(),
            in_order: true,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.headless() {
            // A headless run has no sleeping loop; pace it in real time so
            // a pair of headless peers has time to talk.
            std::thread::sleep(Duration::from_millis(16));
            ctx.request_redraw();
        }
        self.poll_host();
        self.poll_guest(ctx);
        if ctx.input.mouse_inside() {
            self.share_pointer(ctx.input.mouse());
        }
        self.draft.push_str(ctx.input.text());
        if (ctx.input.key_pressed(Key::Backspace) || ctx.input.key_repeated(Key::Backspace))
            && !self.draft.is_empty()
        {
            self.draft.pop();
        }
        if ctx.input.key_pressed(Key::Enter) && !self.draft.is_empty() {
            let text = std::mem::take(&mut self.draft);
            self.say(format!("{}: {text}", self.name));
            self.send_chat(text);
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        for (name, p) in &self.points {
            ctx.gfx.fill_rect(
                Rect::new(p.x - 5.0, p.y - 5.0, 10.0, 10.0),
                Color::rgb8(255, 200, 80),
            );
            common::label(
                ctx,
                &self.font,
                name,
                *p + vec2(8.0, -8.0),
                14.0,
                Color::rgb8(255, 220, 140),
            );
        }
        common::panel(ctx, Rect::new(16.0, 16.0, 480.0, 380.0));
        for (i, line) in self.lines.iter().enumerate() {
            common::label(
                ctx,
                &self.font,
                line,
                vec2(28.0, 24.0 + i as f32 * 24.0),
                15.0,
                common::TEXT,
            );
        }
        common::panel(ctx, Rect::new(16.0, 404.0, 480.0, 52.0));
        let prompt = if self.draft.is_empty() {
            ("Type and press Enter".to_owned(), common::DIM)
        } else {
            (format!("{}|", self.draft), common::TEXT)
        };
        common::label(
            ctx,
            &self.font,
            &prompt.0,
            vec2(28.0, 421.0),
            15.0,
            prompt.1,
        );
        if self.reliable {
            common::panel(ctx, Rect::new(512.0, 16.0, 192.0, 120.0));
            let order = if self.in_order {
                "in order"
            } else {
                "OUT OF ORDER"
            };
            let mut rows = vec![
                "Reliable UDP".to_owned(),
                format!("Sent {}", self.sent),
                format!("Received {}", self.received),
                order.to_owned(),
            ];
            if let Some((rtt, loss)) = self.link_stats() {
                rows.push(format!(
                    "rtt {:.1} ms loss {:.0}%",
                    rtt.as_secs_f64() * 1000.0,
                    loss * 100.0
                ));
            }
            for (i, row) in rows.iter().enumerate() {
                common::label(
                    ctx,
                    &self.font,
                    row,
                    vec2(524.0, 24.0 + i as f32 * 20.0),
                    14.0,
                    common::TEXT,
                );
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let mut config = Config::new("Yowie network").with_size(720, 480);
    config.loop_mode = LoopMode::TurnBased;
    yowie::run::<Chat>(config)
}
