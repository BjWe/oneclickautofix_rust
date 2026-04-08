use std::fs;
use std::path::Path;
use std::process::Command as Proc;
use std::os::windows::process::CommandExt;

use crate::command::{Command, RegOp};

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn execute(cmd: &Command, on_countdown: Option<&dyn Fn(u64)>) -> Result<(), String> {
    match cmd {
        Command::Taskkill { my_only, pattern } => taskkill(*my_only, pattern),
        Command::Remove   { force, path }      => remove(*force, path),
        Command::Rename   { src, dest }        => rename(src, dest),
        Command::Touch    { path }             => touch(path),
        Command::Copy     { src, dest }        => copy(src, dest),
        Command::Reg      { op, reg_path, args } => reg(op, reg_path, args.as_deref()),
        Command::Logoff   { seconds }          => logoff(*seconds, on_countdown),
        Command::Exec     { executable, params } => exec(executable, params),
    }
}

// ---------------------------------------------------------------------------

fn run_ps(script: &str) -> Result<(), String> {
    let out = Proc::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell konnte nicht gestartet werden: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "PowerShell-Fehler ({}): {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn run_cmd(exe: &str, args: &[&str]) -> Result<(), String> {
    let out = Proc::new(exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("{exe} konnte nicht gestartet werden: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{exe} fehlgeschlagen ({}): {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Wraps a string in PowerShell single quotes (escaping internal `'` as `''`).
fn psq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

// ---------------------------------------------------------------------------

fn taskkill(my_only: bool, pattern: &str) -> Result<(), String> {
    let safe = pattern.replace('\'', "\\'");
    let script = if my_only {
        format!(
            "$pat='{safe}'; $u=$env:USERNAME; \
             Get-WmiObject Win32_Process | \
             Where-Object {{ $_.Name -match $pat }} | \
             ForEach-Object {{ \
               try {{ $o=$_.GetOwner(); \
                 if ($o.User -eq $u) {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }} \
               }} catch {{}} \
             }}"
        )
    } else {
        format!(
            "$pat='{safe}'; \
             Get-WmiObject Win32_Process | \
             Where-Object {{ $_.Name -match $pat }} | \
             ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
        )
    };
    run_ps(&script)
}

fn remove(force: bool, path: &str) -> Result<(), String> {
    if force {
        // PowerShell handles wildcards, missing paths, and locked files better than Rust fs
        return run_ps(&format!(
            "Remove-Item -Path {} -Recurse -Force -ErrorAction SilentlyContinue",
            psq(path)
        ));
    }
    let p = Path::new(path);
    if p.is_dir() {
        fs::remove_dir_all(p).map_err(|e| e.to_string())
    } else if p.exists() {
        fs::remove_file(p).map_err(|e| e.to_string())
    } else {
        Ok(()) // already gone
    }
}

fn rename(src: &str, dest: &str) -> Result<(), String> {
    if !Path::new(src).exists() {
        return Err(format!("Quelle nicht gefunden: {src}"));
    }
    fs::rename(src, dest).map_err(|e| e.to_string())
}

fn touch(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.exists() {
        filetime::set_file_mtime(p, filetime::FileTime::now())
            .map_err(|e| e.to_string())
    } else {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::File::create(p).map(|_| ()).map_err(|e| e.to_string())
    }
}

fn copy(src: &str, dest: &str) -> Result<(), String> {
    let sp = Path::new(src);
    if sp.is_dir() {
        copy_dir(sp, Path::new(dest))
    } else if sp.exists() {
        let dp = Path::new(dest);
        if let Some(parent) = dp.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(sp, dp).map(|_| ()).map_err(|e| e.to_string())
    } else {
        Err(format!("Quelle nicht gefunden: {src}"))
    }
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry  = entry.map_err(|e| e.to_string())?;
        let target = dest.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .map(|_| ())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn reg(op: &RegOp, reg_path: &str, args: Option<&str>) -> Result<(), String> {
    let wp = reg_path.replace('/', "\\");
    match op {
        RegOp::Add => {
            if let Some(a) = args.filter(|a| a.contains('=')) {
                let (name, data) = a.split_once('=').unwrap();
                run_cmd("reg", &["add", &wp, "/v", name, "/d", data, "/f"])
            } else {
                run_cmd("reg", &["add", &wp, "/f"])
            }
        }
        RegOp::Set => {
            let a = args.ok_or("reg:set erfordert ValueName=Daten")?;
            let (name, data) = a.split_once('=')
                .ok_or("reg:set erfordert Format ValueName=Daten")?;
            run_cmd("reg", &["add", &wp, "/v", name, "/d", data, "/f"])
        }
        RegOp::Del => {
            if let Some(name) = args.filter(|s| !s.is_empty()) {
                run_cmd("reg", &["delete", &wp, "/v", name, "/f"])
            } else {
                run_cmd("reg", &["delete", &wp, "/f"])
            }
        }
        RegOp::Rename => {
            let a = args.ok_or("reg:rename erfordert alten und neuen Namen")?;
            let (old, new) = a.split_once(' ')
                .ok_or("reg:rename erfordert alten und neuen Namen")?;
            run_ps(&format!(
                "$p={p}; $o={o}; $n={n}; \
                 $v=(Get-ItemProperty -Path $p -Name $o -ErrorAction Stop).$o; \
                 New-ItemProperty -Path $p -Name $n -Value $v -Force | Out-Null; \
                 Remove-ItemProperty -Path $p -Name $o -Force",
                p = psq(&format!("Registry::{wp}")),
                o = psq(old),
                n = psq(new),
            ))
        }
    }
}

fn logoff(seconds: u64, on_countdown: Option<&dyn Fn(u64)>) -> Result<(), String> {
    for i in (1..=seconds).rev() {
        if let Some(f) = on_countdown { f(i); }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    if let Some(f) = on_countdown { f(0); }
    run_cmd("shutdown", &["/l"])
}

fn exec(executable: &str, params: &[String]) -> Result<(), String> {
    let args: Vec<&str> = params.iter().map(|s| s.as_str()).collect();
    let out = Proc::new(executable)
        .args(&args)
        .output()
        .map_err(|e| format!("{executable} konnte nicht gestartet werden: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Prozess beendet mit Code {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}
