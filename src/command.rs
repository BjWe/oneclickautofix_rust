use std::env;
use regex::Regex;

#[derive(Debug, Clone)]
pub enum Command {
    Taskkill { my_only: bool, pattern: String },
    Remove   { force: bool, path: String },
    Rename   { src: String, dest: String },
    Touch    { path: String },
    Copy     { src: String, dest: String },
    Reg      { op: RegOp, reg_path: String, args: Option<String> },
    Logoff   { seconds: u64 },
    Exec     { executable: String, params: Vec<String> },
}

#[derive(Debug, Clone)]
pub enum RegOp { Add, Del, Set, Rename }

pub fn parse_command(raw: &str) -> Result<Command, String> {
    let resolved = resolve_env_vars(raw.trim());
    let s = resolved.as_str();

    // taskkill:<my|all> <PATTERN>
    if let Some(rest) = s.strip_prefix("taskkill:") {
        let (mode, pat) = rest.split_once(' ')
            .ok_or_else(|| format!("Ungültiger taskkill-Befehl: {raw}"))?;
        return Ok(Command::Taskkill { my_only: mode == "my", pattern: pat.to_string() });
    }

    // remove:force <PATH> | remove: <PATH> | remove <PATH>
    if let Some(rest) = s.strip_prefix("remove:force ") {
        let path = first_arg(rest).ok_or("remove:force: kein Pfad")?;
        return Ok(Command::Remove { force: true, path });
    }
    if let Some(rest) = s.strip_prefix("remove: ").or_else(|| s.strip_prefix("remove ")) {
        let path = first_arg(rest).ok_or("remove: kein Pfad")?;
        return Ok(Command::Remove { force: false, path });
    }

    // rename 'SRC' 'DEST'  (single-quotes recommended for paths with spaces)
    if let Some(rest) = s.strip_prefix("rename ") {
        let parts = split_args(rest);
        if parts.len() >= 2 {
            return Ok(Command::Rename { src: parts[0].clone(), dest: parts[1].clone() });
        }
        // Fallback: split at space before next absolute path
        let (src, dest) = split_two_paths(rest)
            .ok_or_else(|| format!("rename: Pfade nicht erkennbar: {raw}"))?;
        return Ok(Command::Rename { src, dest });
    }

    // touch <PATH>
    if let Some(rest) = s.strip_prefix("touch ") {
        let path = first_arg(rest).ok_or("touch: kein Pfad")?;
        return Ok(Command::Touch { path });
    }

    // copy <SRC> <DEST>
    if let Some(rest) = s.strip_prefix("copy ") {
        let parts = split_args(rest);
        if parts.len() < 2 {
            return Err(format!("copy: Quelle und Ziel erforderlich: {raw}"));
        }
        return Ok(Command::Copy { src: parts[0].clone(), dest: parts[1].clone() });
    }

    // reg:<add|del|set|rename> <REGPATH> [ARGS]
    if let Some(rest) = s.strip_prefix("reg:") {
        let (op_str, rest) = rest.split_once(' ')
            .ok_or_else(|| format!("Ungültiger reg-Befehl: {raw}"))?;
        let op = match op_str {
            "add"    => RegOp::Add,
            "del"    => RegOp::Del,
            "set"    => RegOp::Set,
            "rename" => RegOp::Rename,
            _        => return Err(format!("Unbekannte reg-Operation: {op_str}")),
        };
        let mut it = rest.splitn(2, ' ');
        let reg_path = it.next().unwrap().to_string();
        let args = it.next().filter(|s| !s.is_empty()).map(str::to_string);
        return Ok(Command::Reg { op, reg_path, args });
    }

    // logoff <SECONDS>
    if let Some(rest) = s.strip_prefix("logoff ") {
        let sec = rest.trim().parse::<u64>()
            .map_err(|_| format!("logoff: ungültige Sekunden: {rest}"))?;
        return Ok(Command::Logoff { seconds: sec });
    }

    // exec <PATH> [PARAMS...]
    if let Some(rest) = s.strip_prefix("exec ") {
        let parts = split_args(rest);
        if parts.is_empty() {
            return Err(format!("exec: kein Programm angegeben: {raw}"));
        }
        return Ok(Command::Exec { executable: parts[0].clone(), params: parts[1..].to_vec() });
    }

    Err(format!("Unbekannter Befehl: {raw}"))
}

// ---------------------------------------------------------------------------

fn resolve_env_vars(input: &str) -> String {
    let re = Regex::new(r"%(\w+)%").unwrap();
    re.replace_all(input, |caps: &regex::Captures| {
        let name = &caps[1];
        env::var(name)
            .or_else(|_| env::var(name.to_uppercase()))
            .unwrap_or_else(|_| caps[0].to_string())
    })
    .into_owned()
}

/// Splits by whitespace, honouring single- and double-quoted tokens (quotes stripped).
pub fn split_args(input: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut buf    = String::new();
    let mut quote: Option<char> = None;

    for c in input.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_)           => buf.push(c),
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ' ' => {
                if !buf.is_empty() {
                    result.push(std::mem::take(&mut buf));
                }
            }
            None => buf.push(c),
        }
    }
    if !buf.is_empty() {
        result.push(buf);
    }
    result
}

fn first_arg(s: &str) -> Option<String> {
    split_args(s).into_iter().next()
}

/// Splits "SRC DEST" where paths may contain spaces (unquoted fallback).
/// Splits at the first space that is followed by an absolute-path start.
fn split_two_paths(input: &str) -> Option<(String, String)> {
    let re = Regex::new(r" (?=%[\w]+%|[A-Za-z]:[/\\]|[/\\]{2})").unwrap();
    let m = re.find(input)?;
    Some((
        input[..m.start()].trim().to_string(),
        input[m.end()..].trim().to_string(),
    ))
}
