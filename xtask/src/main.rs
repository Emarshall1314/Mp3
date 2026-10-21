//! `cargo xtask <command>`: project automation that would otherwise live in shell scripts.
//!
//! Commands:
//!   fixtures [--out DIR] [--only GROUP...]  generate the test library (needs ffmpeg, sox, flac,
//!                                           lame, opusenc, oggenc, wavpack, python3)
//!   test                                    Rust + UI tests, then the fixture self-check
//!   bench                                   Rust benchmarks
//!   ci                                      everything CI runs: fmt, clippy, lint, typecheck, tests

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((cmd, rest)) => (cmd.as_str(), rest),
        None => {
            eprintln!("usage: cargo xtask <fixtures|test|bench|ci> [args]");
            return ExitCode::from(2);
        }
    };
    let result = match cmd {
        "fixtures" => fixtures(rest),
        "test" => test(),
        "bench" => run(Command::new("cargo").args(["bench", "--workspace"])),
        "ci" => ci(),
        other => Err(format!("unknown command `{other}`")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask: {err}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    // xtask/ lives directly under the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Default fixture output directory; tests find it through `CADENZA_FIXTURES` or this path.
pub fn fixtures_dir() -> PathBuf {
    root().join("fixtures-out")
}

fn run(cmd: &mut Command) -> Result {
    let shown = format!("{cmd:?}");
    eprintln!("» {shown}");
    let status = cmd
        .current_dir(root())
        .status()
        .map_err(|e| format!("{shown}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{shown} failed: {status}"))
    }
}

/// Creates (once) a virtualenv with the fixture generator's pinned Python dependencies.
fn python_env() -> Result<PathBuf> {
    let venv = root().join("target/fixtures-venv");
    let python = venv.join("bin/python");
    let requirements = root().join("tools/fixtures/requirements.txt");
    let stamp = venv.join(".requirements");
    let wanted = std::fs::read_to_string(&requirements).map_err(|e| e.to_string())?;
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(wanted.as_str()) {
        run(Command::new("python3").args(["-m", "venv"]).arg(&venv))?;
        run(Command::new(&python)
            .args(["-m", "pip", "install", "-q", "-r"])
            .arg(&requirements))?;
        std::fs::write(&stamp, wanted).map_err(|e| e.to_string())?;
    }
    Ok(python)
}

fn fixtures(args: &[String]) -> Result {
    let python = python_env()?;
    let mut cmd = Command::new(python);
    // -I: isolated mode, so nothing from the current directory shadows stdlib modules.
    cmd.arg("-I").arg(root().join("tools/fixtures/generate.py"));
    if !args.iter().any(|a| a == "--out") {
        cmd.arg("--out").arg(fixtures_dir());
    }
    run(cmd.args(args))
}

fn fixture_check() -> Result {
    let python = python_env()?;
    run(Command::new(python)
        .arg("-I")
        .arg(root().join("tools/fixtures/check.py"))
        .arg(fixtures_dir()))
}

fn test() -> Result {
    run(Command::new("cargo").args(["test", "--workspace"]))?;
    run(Command::new("pnpm").args(["-r", "test"]))?;
    if fixtures_dir().join("manifest.json").exists() {
        fixture_check()?;
    } else {
        eprintln!(
            "(fixtures not generated; run `cargo xtask fixtures` to include the fixture self-check)"
        );
    }
    Ok(())
}

fn ci() -> Result {
    run(Command::new("cargo").args(["fmt", "--all", "--check"]))?;
    run(Command::new("cargo").args([
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ]))?;
    run(Command::new("pnpm").args(["format:check"]))?;
    run(Command::new("pnpm").args(["-r", "lint"]))?;
    run(Command::new("pnpm").args(["-r", "typecheck"]))?;
    fixtures(&[])?;
    test()
}
