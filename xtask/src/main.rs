//! `cargo xtask <command>`: project automation that would otherwise live in shell scripts.
//!
//! Commands:
//!   fixtures [--out DIR] [--only GROUP...]  generate and self-check the test library (needs
//!                                           ffmpeg, sox, flac, lame, opusenc, oggenc, wavpack,
//!                                           python3, git, cmake)
//!   test                                    Rust + UI tests (generates fixtures first if missing;
//!                                           starts local WebDAV and S3 servers for the
//!                                           remote-store tests, or skips those if it can't)
//!   bench                                   performance budgets on a 200k-track library
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
        "bench" => bench(),
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
    venv("fixtures-venv", "tools/fixtures/requirements.txt")
}

/// Creates (once) a virtualenv in `target/<name>` with pinned requirements.
fn venv(name: &str, requirements: &str) -> Result<PathBuf> {
    let venv = root().join("target").join(name);
    let python = venv.join("bin/python");
    let requirements = root().join(requirements);
    let stamp = venv.join(".requirements");
    let wanted = std::fs::read_to_string(&requirements).map_err(|e| e.to_string())?;
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(wanted.as_str()) {
        // --clear: a partly restored CI cache must not leave stale packages behind.
        run(Command::new("python3")
            .args(["-m", "venv", "--clear"])
            .arg(&venv))?;
        run(Command::new(&python)
            .args(["-m", "pip", "install", "-q", "-r"])
            .arg(&requirements))?;
        std::fs::write(&stamp, wanted).map_err(|e| e.to_string())?;
    }
    Ok(python)
}

/// Monkey's Audio has no distro package; build the encoder used for APE fixtures from a pinned
/// commit of the Unix port. It is a test-time tool only and never linked into Cadenza.
const MAC_REPO: &str = "https://github.com/fernandotcl/monkeys-audio";
const MAC_COMMIT: &str = "5fe956c7e67c13daa80518a4cc7001e9fa185297";

fn monkeys_audio() -> Result<PathBuf> {
    let src = root().join("target/tools/monkeys-audio");
    let bin = src.join("build/mac");
    if bin.exists() {
        return Ok(bin);
    }
    // Start from a clean clone: a CI cache can restore the directory without its build or `.git`.
    if src.exists() {
        std::fs::remove_dir_all(&src).map_err(|e| e.to_string())?;
    }
    run(Command::new("git")
        .args(["clone", "--quiet", MAC_REPO])
        .arg(&src))?;
    run(Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["checkout", "--quiet", MAC_COMMIT]))?;
    run(Command::new("cmake")
        .arg("-S")
        .arg(&src)
        .arg("-B")
        .arg(src.join("build"))
        .arg("-DCMAKE_BUILD_TYPE=Release"))?;
    run(Command::new("cmake")
        .arg("--build")
        .arg(src.join("build"))
        .arg("-j"))?;
    Ok(bin)
}

fn fixtures(args: &[String]) -> Result {
    let python = python_env()?;
    let mut cmd = Command::new(&python);
    cmd.env("CADENZA_MAC", monkeys_audio()?);
    // -I: isolated mode, so nothing from the current directory shadows stdlib modules.
    cmd.arg("-I").arg(root().join("tools/fixtures/generate.py"));
    let out = match args.iter().position(|a| a == "--out") {
        Some(i) => PathBuf::from(args.get(i + 1).ok_or("--out needs a directory")?),
        None => {
            cmd.arg("--out").arg(fixtures_dir());
            fixtures_dir()
        }
    };
    run(cmd.args(args))?;
    // Never hand tests a library that doesn't match its own manifest.
    run(Command::new(python)
        .arg("-I")
        .arg(root().join("tools/fixtures/check.py"))
        .arg(out))
}

/// Local WebDAV and S3 servers for the remote-store tests; stopped when dropped.
struct TestServers {
    children: Vec<std::process::Child>,
    env: Vec<(String, String)>,
}

impl Drop for TestServers {
    fn drop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn free_port() -> Result<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| e.to_string())
}

fn wait_for(port: u16) -> Result {
    let start = std::time::Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        if start.elapsed() > std::time::Duration::from_secs(30) {
            return Err(format!("test server on port {port} didn't start"));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(())
}

fn test_servers() -> Result<TestServers> {
    let python = venv("servers-venv", "tools/test-servers/requirements.txt")?;
    let bin = python.parent().ok_or("no venv bin")?.to_path_buf();
    let dir = root().join("target/test-servers");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("dav")).map_err(|e| e.to_string())?;
    let (dav_port, s3_port) = (free_port()?, free_port()?);
    let config = format!(
        "host: 127.0.0.1\nport: {dav_port}\nprovider_mapping:\n  \"/dav\": {dav}\n\
         http_authenticator:\n  accept_basic: true\n  accept_digest: false\n  default_to_digest: false\n\
         simple_dc:\n  user_mapping:\n    \"*\":\n      \"owner\":\n        password: \"secret-pass\"\nverbose: 0\n",
        dav = dir.join("dav").display()
    );
    std::fs::write(dir.join("wsgidav.yaml"), config).map_err(|e| e.to_string())?;
    let log = |name: &str| std::fs::File::create(dir.join(name)).map_err(|e| e.to_string());
    let mut servers = TestServers {
        children: Vec::new(),
        env: Vec::new(),
    };
    servers.children.push(
        Command::new(bin.join("wsgidav"))
            .arg("--config")
            .arg(dir.join("wsgidav.yaml"))
            .stdout(log("dav.log")?)
            .stderr(log("dav.err")?)
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    servers.children.push(
        Command::new(bin.join("moto_server"))
            .args(["-H", "127.0.0.1", "-p", &s3_port.to_string()])
            .stdout(log("s3.log")?)
            .stderr(log("s3.err")?)
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    wait_for(dav_port)?;
    wait_for(s3_port)?;
    let endpoint = format!("http://127.0.0.1:{s3_port}");
    run(Command::new(&python)
        .arg("-I")
        .arg("-c")
        .arg(format!(
            "import boto3; boto3.client('s3', endpoint_url='{endpoint}', aws_access_key_id='test', \
             aws_secret_access_key='test', region_name='us-east-1').create_bucket(Bucket='cadenza')"
        ))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost"))?;
    servers.env = vec![
        (
            "CADENZA_TEST_WEBDAV".into(),
            format!("http://127.0.0.1:{dav_port}/dav|owner|secret-pass"),
        ),
        (
            "CADENZA_TEST_S3".into(),
            format!("{endpoint}|cadenza|test|test"),
        ),
    ];
    Ok(servers)
}

/// `require_servers`: fail rather than skip the remote-store tests when the local servers can't
/// start (CI); otherwise they're skipped with a note.
fn test_with(require_servers: bool) -> Result {
    if !fixtures_dir().join("manifest.json").exists() {
        fixtures(&[])?;
    }
    let servers = match test_servers() {
        Ok(s) => Some(s),
        Err(e) if !require_servers => {
            eprintln!("xtask: remote-store tests will be skipped: {e}");
            None
        }
        Err(e) => return Err(e),
    };
    let no_proxy = |var: &str| {
        let old = std::env::var(var).unwrap_or_default();
        format!("127.0.0.1,localhost,{old}")
    };
    let mut cargo = Command::new("cargo");
    cargo
        .args(["test", "--workspace"])
        .env("CADENZA_FIXTURES", fixtures_dir())
        .env("NO_PROXY", no_proxy("NO_PROXY"))
        .env("no_proxy", no_proxy("no_proxy"));
    for (k, v) in servers.iter().flat_map(|s| s.env.iter()) {
        cargo.env(k, v);
    }
    run(&mut cargo)?;
    drop(servers);
    run(Command::new("pnpm").args(["-r", "test"]))
}

fn test() -> Result {
    test_with(false)
}

/// Performance budgets at library scale (`CADENZA_PERF_TRACKS`, default 200 000).
fn bench() -> Result {
    run(Command::new("cargo").args([
        "test",
        "--release",
        "-p",
        "cadenza-library",
        "--test",
        "perf",
        "--",
        "--ignored",
        "--nocapture",
    ]))
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
    // Regenerate from scratch so CI exercises the generator and its self-check every time.
    fixtures(&[])?;
    test_with(true)
}
