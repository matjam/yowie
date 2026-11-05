//! Running command lines and the built-in commands.

use log::LevelFilter;
use yowie_input::Key;

use crate::complete::{Completion, Names, complete};
use crate::console::{CommandResult, Core, Request};
use crate::logcap::LogLine;
use crate::parse::split_words;

/// How deep `exec` files may run other `exec` files.
const MAX_EXEC_DEPTH: usize = 8;

/// The built-in commands and their descriptions.
pub(crate) const BUILTINS: &[(&str, &str)] = &[
    ("help", "help [command]: list the commands, or describe one"),
    ("echo", "echo <text...>: print the arguments"),
    ("clear", "clear: empty the output"),
    ("quit", "quit: end the game"),
    ("screenshot", "screenshot [file]: write the next frame to a PNG (screenshot.png by default)"),
    ("exec", "exec <file>: run a file of commands, one per line (# and // start comments)"),
    ("bind", "bind <key> <command...>: run a command when a key is pressed"),
    ("unbind", "unbind <key>: drop a key's binding"),
    ("binds", "binds: list the key bindings"),
    ("fps", "fps: the frame rate and frame time"),
    ("stats", "stats: frame timings and draw counts"),
    ("log", "log [level]: show log records at this level and above: debug, info, warn or error"),
    ("panels", "panels [tab]: open or close the debug panels, on a named tab"),
    ("set", "set <name> <value>: change a variable"),
    ("get", "get <name>: print a variable"),
    ("vars", "vars [prefix]: list the variables"),
];

impl Core {
    /// Runs a typed line: echoes it, records it in the history and runs it.
    pub(crate) fn execute(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.capture.push(LogLine {
            level: log::Level::Trace,
            target: "input".into(),
            text: format!("> {line}"),
        });
        self.history.push(line);
        self.run_line(line, 0);
    }

    /// Runs a line without echoing it, as `exec` files and bindings do.
    pub(crate) fn run_line(&mut self, line: &str, depth: usize) {
        let words = match split_words(line) {
            Ok(w) => w,
            Err(e) => {
                self.print_error(&format!("{e} in {line:?}"));
                return;
            }
        };
        if words.is_empty() {
            return;
        }
        match self.dispatch(&words, depth) {
            Ok(text) if !text.is_empty() => self.print(&text),
            Ok(_) => {}
            Err(e) => self.print_error(&e),
        }
    }

    fn dispatch(&mut self, words: &[String], depth: usize) -> CommandResult {
        let name = words[0].as_str();
        let args = &words[1..];
        if let Some(cmd) = self.commands.get_mut(name) {
            return (cmd.run)(args);
        }
        if let Some(r) = self.builtin(name, args, depth) {
            return r;
        }
        if let Some(v) = self.vars.get(name) {
            if args.is_empty() {
                return Ok(format!("{name} = {}", v.show()));
            }
            v.assign(&args.join(" "))?;
            return Ok(format!("{name} = {}", v.show()));
        }
        Err(format!("unknown command {name:?}; try help"))
    }

    fn builtin(&mut self, name: &str, args: &[String], depth: usize) -> Option<CommandResult> {
        Some(match name {
            "help" => self.cmd_help(args),
            "echo" => Ok(args.join(" ")),
            "clear" => {
                self.capture.clear();
                Ok(String::new())
            }
            "quit" => {
                self.requests.push(Request::Quit);
                Ok("quitting".into())
            }
            "screenshot" => {
                let path = args.first().map_or("screenshot.png", String::as_str).to_owned();
                self.requests.push(Request::Screenshot(path.clone()));
                Ok(format!("saving {path}"))
            }
            "exec" => self.cmd_exec(args, depth),
            "bind" => self.cmd_bind(args),
            "unbind" => match args {
                [k] => parse_key(k).and_then(|k| {
                    if self.binds.remove(&k).is_some() {
                        Ok(format!("unbound {k}"))
                    } else {
                        Err(format!("{k} is not bound"))
                    }
                }),
                _ => Err("usage: unbind <key>".into()),
            },
            "binds" => Ok(self
                .binds
                .iter()
                .map(|(k, c)| format!("{k} = {c}"))
                .collect::<Vec<_>>()
                .join("\n")),
            "fps" => Ok(self.cmd_fps()),
            "stats" => Ok(self.cmd_stats()),
            "log" => self.cmd_log(args),
            "panels" => self.cmd_panels(args),
            "set" => match args {
                [n, rest @ ..] if !rest.is_empty() => match self.vars.get(n.as_str()) {
                    Some(v) => v.assign(&rest.join(" ")).map(|()| format!("{n} = {}", v.show())),
                    None => Err(format!("no variable {n:?}")),
                },
                _ => Err("usage: set <name> <value>".into()),
            },
            "get" => match args {
                [n] => match self.vars.get(n.as_str()) {
                    Some(v) => Ok(format!("{n} = {}", v.show())),
                    None => Err(format!("no variable {n:?}")),
                },
                _ => Err("usage: get <name>".into()),
            },
            "vars" => {
                let prefix = args.first().map_or("", String::as_str);
                let rows: Vec<String> = self
                    .vars
                    .iter()
                    .filter(|(n, _)| n.starts_with(prefix))
                    .map(|(n, v)| {
                        let help = if v.help.is_empty() { String::new() } else { format!("  {}", v.help) };
                        format!("{n} = {} ({}){help}", v.show(), v.kind())
                    })
                    .collect();
                if rows.is_empty() { Err("no variables".into()) } else { Ok(rows.join("\n")) }
            }
            _ => return None,
        })
    }

    fn cmd_help(&self, args: &[String]) -> CommandResult {
        if let Some(name) = args.first() {
            if let Some(c) = self.commands.get(name.as_str()) {
                return Ok(format!("{name}: {}", c.help));
            }
            if let Some((_, h)) = BUILTINS.iter().find(|(n, _)| n == name) {
                return Ok((*h).to_owned());
            }
            if let Some(v) = self.vars.get(name.as_str()) {
                return Ok(format!("{name} ({}): {}", v.kind(), v.help));
            }
            return Err(format!("no command {name:?}"));
        }
        let mut rows: Vec<(String, String)> = BUILTINS
            .iter()
            .filter(|(n, _)| !self.commands.contains_key(*n))
            .map(|(n, h)| ((*n).to_owned(), (*h).to_owned()))
            .collect();
        rows.extend(self.commands.iter().map(|(n, c)| (n.clone(), c.help.clone())));
        rows.sort();
        let mut out = rows
            .iter()
            .map(|(n, h)| format!("{n:<12} {h}"))
            .collect::<Vec<_>>()
            .join("\n");
        if !self.vars.is_empty() {
            out.push_str(&format!("\n{} variables: see vars", self.vars.len()));
        }
        Ok(out)
    }

    fn cmd_exec(&mut self, args: &[String], depth: usize) -> CommandResult {
        let [path] = args else {
            return Err("usage: exec <file>".into());
        };
        if depth >= MAX_EXEC_DEPTH {
            return Err("exec files nest too deeply".into());
        }
        let text = match &self.opts.reader {
            Some(read) => read(path)?,
            None => std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
        };
        for line in text.lines() {
            self.run_line(line, depth + 1);
        }
        Ok(String::new())
    }

    fn cmd_bind(&mut self, args: &[String]) -> CommandResult {
        let [key, cmd @ ..] = args else {
            return Err("usage: bind <key> <command...>".into());
        };
        if cmd.is_empty() {
            return Err("usage: bind <key> <command...>".into());
        }
        let k = parse_key(key)?;
        let line = cmd
            .iter()
            .map(|w| if w.contains(' ') || w.is_empty() { format!("\"{w}\"") } else { w.clone() })
            .collect::<Vec<_>>()
            .join(" ");
        self.binds.insert(k, line.clone());
        Ok(format!("{k} = {line}"))
    }

    fn cmd_fps(&self) -> String {
        match self.frames.back() {
            Some(t) => format!("{:.1} fps, {:.2} ms per frame", t.fps, t.frame_ms),
            None => "no frames recorded yet".into(),
        }
    }

    fn cmd_stats(&self) -> String {
        let Some(t) = self.frames.back() else {
            return "no frames recorded yet".into();
        };
        let s = self.gpu.stats;
        format!(
            "{:.1} fps  frame {:.2} ms  update {:.2} ms  draw {:.2} ms\n{} draw calls  {} vertices  {} indices  {} outputs",
            t.fps, t.frame_ms, t.update_ms, t.draw_ms, s.draw_calls, s.vertices, s.indices, s.outputs
        )
    }

    fn cmd_log(&mut self, args: &[String]) -> CommandResult {
        let Some(level) = args.first() else {
            return Ok(format!("log level {}", self.capture.level()));
        };
        let filter = match level.to_ascii_lowercase().as_str() {
            "debug" => LevelFilter::Debug,
            "info" => LevelFilter::Info,
            "warn" | "warning" => LevelFilter::Warn,
            "error" => LevelFilter::Error,
            "trace" => LevelFilter::Trace,
            other => return Err(format!("unknown level {other:?}: use debug, info, warn or error")),
        };
        self.capture.set_level(filter);
        Ok(format!("log level {filter}"))
    }

    fn cmd_panels(&mut self, args: &[String]) -> CommandResult {
        match args.first() {
            None => {
                self.panels_open = !self.panels_open;
                Ok(String::new())
            }
            Some(name) => {
                let names = self.panel_names();
                match names.iter().position(|n| n.eq_ignore_ascii_case(name)) {
                    Some(i) => {
                        self.tab = i;
                        self.panels_open = true;
                        Ok(String::new())
                    }
                    None => Err(format!("no panel {name:?}; panels: {}", names.join(", "))),
                }
            }
        }
    }

    /// Every name Tab can complete from, for the line being typed.
    pub(crate) fn complete_line(&self) -> Completion {
        let mut commands: Vec<String> = BUILTINS.iter().map(|(n, _)| (*n).to_owned()).collect();
        commands.extend(self.commands.keys().cloned());
        commands.sort();
        commands.dedup();
        let vars: Vec<String> = self.vars.keys().cloned().collect();
        let keys: Vec<&str> = Key::ALL.iter().map(|k| k.name()).collect();
        let panels = self.panel_names();
        let names = Names {
            commands: &commands,
            vars: &vars,
            keys: &keys,
            panels: &panels,
        };
        complete(&self.line, &names, &|cmd, args| {
            self.commands.get(cmd).and_then(|c| c.complete.as_ref()).map(|f| f(args))
        })
    }
}

fn parse_key(name: &str) -> Result<Key, String> {
    Key::from_name(name).ok_or_else(|| format!("unknown key {name:?}"))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::console::{Console, ConsoleOptions};
    use crate::logcap::LogLine;

    use super::*;

    fn console() -> Console {
        let mut c = Console::new(ConsoleOptions::default());
        c.core.capture.clear();
        c
    }

    fn out(c: &Console) -> Vec<String> {
        c.core.capture.window(100, 0).into_iter().map(|l: LogLine| l.text).collect()
    }

    #[test]
    fn custom_commands_get_parsed_arguments() {
        let mut c = console();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        c.register("give", "give <item> [count]", move |args| {
            *s.borrow_mut() = args.to_vec();
            Ok(format!("gave {}", args.join("+")))
        });
        c.execute("give \"iron sword\" 2");
        assert_eq!(*seen.borrow(), ["iron sword", "2"]);
        assert_eq!(out(&c), ["> give \"iron sword\" 2", "gave iron sword+2"]);
        c.register("fail", "", |_| Err("nope".into()));
        c.execute("fail");
        assert_eq!(out(&c).last().map(String::as_str), Some("nope"));
        c.execute("nonsense");
        assert!(out(&c).last().is_some_and(|l| l.contains("unknown command")));
    }

    #[test]
    fn variables_are_typed_and_shared_with_the_game() {
        let mut c = console();
        let speed = c.float("player.speed", 5.0, "how fast");
        let noclip = c.boolean("player.noclip", false, "");
        let name = c.string("player.name", "Ada", "");
        c.execute("set player.speed 8.5");
        c.execute("set player.noclip on");
        c.execute("set player.name Grace Hopper");
        assert_eq!(speed.get(), 8.5);
        assert!(noclip.get());
        assert_eq!(name.get(), "Grace Hopper");
        c.execute("get player.speed");
        c.execute("set player.speed fast");
        c.execute("player.speed");
        c.execute("player.speed 2");
        assert_eq!(speed.get(), 2.0);
        let lines = out(&c);
        assert!(lines.iter().any(|l| l == "player.speed = 8.5"));
        assert!(lines.iter().any(|l| l.contains("not a number")));
        c.execute("vars player.");
        assert!(out(&c).iter().any(|l| l.starts_with("player.noclip = true (bool)")));
    }

    #[test]
    fn bindings_run_lines_and_list() {
        let mut c = console();
        c.execute("bind F5 echo \"jump now\"");
        c.execute("binds");
        assert!(out(&c).iter().any(|l| l == "F5 = echo \"jump now\""));
        assert_eq!(c.core.binds.get(&Key::F5).map(String::as_str), Some("echo \"jump now\""));
        c.execute("bind Nope x");
        assert!(out(&c).last().is_some_and(|l| l.contains("unknown key")));
        c.execute("unbind F5");
        assert!(c.core.binds.is_empty());
    }

    #[test]
    fn exec_runs_files_through_the_reader() {
        let mut c = console();
        c.core.opts.reader = Some(Box::new(|p| {
            if p == "tuning.cfg" {
                Ok("# tuning\nset a 1\n\n// more\necho done\nexec tuning.cfg".into())
            } else {
                Err(format!("{p}: not found"))
            }
        }));
        let a = c.int("a", 0, "");
        c.execute("exec tuning.cfg");
        assert_eq!(a.get(), 1);
        let lines = out(&c);
        assert!(lines.iter().any(|l| l == "done"));
        assert!(lines.iter().any(|l| l.contains("nest too deeply")), "recursion is cut off");
        c.execute("exec missing.cfg");
        assert!(out(&c).last().is_some_and(|l| l.contains("not found")));
    }

    #[test]
    fn requests_and_help() {
        let mut c = console();
        c.execute("screenshot shot.png");
        c.execute("quit");
        assert_eq!(c.take_requests(), [Request::Screenshot("shot.png".into()), Request::Quit]);
        c.register("zap", "zap: do it", |_| Ok(String::new()));
        c.execute("help");
        let lines = out(&c);
        assert!(lines.iter().any(|l| l.starts_with("zap ")));
        assert!(lines.iter().any(|l| l.starts_with("screenshot ")));
        c.execute("help zap");
        assert!(out(&c).iter().any(|l| l == "zap: zap: do it"));
        c.execute("clear");
        assert!(out(&c).is_empty());
    }

    #[test]
    fn user_commands_replace_builtins_and_complete() {
        let mut c = console();
        c.register_completing("spawn", "", |_| Ok(String::new()), |args| {
            if args.len() == 1 { vec!["goblin".into(), "gnome".into()] } else { Vec::new() }
        });
        c.core.line = "spawn go".into();
        assert_eq!(c.core.complete_line().candidates, ["goblin"]);
        c.core.line = "he".into();
        assert_eq!(c.core.complete_line().candidates, ["help"]);
        c.register("echo", "mine", |_| Ok("custom".into()));
        c.execute("echo hi");
        assert_eq!(out(&c).last().map(String::as_str), Some("custom"));
    }
}
