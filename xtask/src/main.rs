use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::{env, fs};

use serde_json::{Value, json};

const USAGE: &str = "usage: cargo xtask header [--check] | wasm | xcframework | dist [<dir>] | pin [--check] [<dir>]";

const NAME: &str = "shapeof";

const NPM: &str = "@meritt/shapeof-wasm";

const PRODUCT: &str = "ShapeOf";

const BINARYEN: &str = "133";

const WASM_OPT_FLAGS: [&str; 3] = ["-O3", "--strip-debug", "--strip-producers"];

const MACOS: &str = "26.0";

const APPLE_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

const CORPUS: &str = "testdata/cases.json";

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["header"] => header(false),
        ["header", "--check"] => header(true),
        ["wasm"] => wasm().map(drop),
        ["xcframework"] => xcframework().map(drop),
        ["dist"] => dist(None),
        ["dist", dir] => dist(Some(Path::new(dir))),
        ["pin"] => pin(false, None),
        ["pin", "--check"] => pin(true, None),
        ["pin", "--check", dir] => pin(true, Some(Path::new(dir))),
        ["pin", dir] => pin(false, Some(Path::new(dir))),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn header(check: bool) -> Result {
    let ffi = root().join(format!("crates/{NAME}-ffi"));
    let path = ffi.join(format!("include/{NAME}.h"));
    let config = cbindgen::Config::from_file(ffi.join("cbindgen.toml"))?;
    let bindings = cbindgen::Builder::new()
        .with_crate(&ffi)
        .with_config(config)
        .generate()
        .map_err(|e| format!("cbindgen: {e}"))?;
    if check {
        let mut fresh = Vec::new();
        bindings.write(&mut fresh);
        if fs::read(&path).map_err(at(path.display()))? != fresh {
            return Err(format!("{} is stale: run `cargo xtask header`", path.display()));
        }
    } else {
        bindings.write_to_file(&path);
    }
    Ok(())
}

fn wasm() -> Result<PathBuf> {
    let root = root();
    let version = version()?;
    let pkg = root.join("packages/npm");
    let manifest = pkg.join("package.json");
    let npm: Value = serde_json::from_slice(&fs::read(&manifest).map_err(at(manifest.display()))?)
        .map_err(at(manifest.display()))?;
    if npm["version"] != version.as_str() {
        return Err(format!("{} has version {}, Cargo.toml {version}", manifest.display(), npm["version"]));
    }

    let opt = env::var_os("WASM_OPT").map_or_else(|| PathBuf::from("wasm-opt"), PathBuf::from);
    let found = capture(Command::new(&opt).arg("--version"))?;
    if found != format!("wasm-opt version {BINARYEN}") && !found.starts_with(&format!("wasm-opt version {BINARYEN} ")) {
        return Err(format!("{} is {found}, the package is built with binaryen {BINARYEN}", opt.display()));
    }

    run(cargo().args([
        "build",
        "-p",
        &format!("{NAME}-ffi"),
        "--release",
        "--locked",
        "--target",
        "wasm32-unknown-unknown",
    ]))?;
    let out = pkg.join("dist");
    fresh_dir(&out)?;
    run(Command::new(&opt)
        .args(WASM_OPT_FLAGS)
        .arg(root.join(format!("target/wasm32-unknown-unknown/release/{NAME}_ffi.wasm")))
        .arg("-o")
        .arg(out.join(format!("{NAME}.wasm"))))?;
    copy(&manifest, &out.join("package.json"))?;
    copy(&root.join("LICENSE"), &out.join("LICENSE"))?;
    let source = pkg.join(format!("{NAME}.js"));
    let module = fs::read_to_string(&source).map_err(at(source.display()))?;
    let mark = "const VERSION = '@VERSION@';";
    if module.matches(mark).count() != 1 {
        return Err(format!("{} must hold `{mark}` once", source.display()));
    }
    write(&out.join(format!("{NAME}.js")), &module.replace(mark, &format!("const VERSION = '{version}';")))?;
    Ok(out)
}

fn xcframework() -> Result<PathBuf> {
    let root = root();
    header(true)?;
    let mut libraries = Vec::new();
    for target in APPLE_TARGETS {
        run(cargo().env("MACOSX_DEPLOYMENT_TARGET", MACOS).args([
            "build",
            "-p",
            &format!("{NAME}-ffi"),
            "--release",
            "--locked",
            "--target",
            target,
        ]))?;
        libraries.push(root.join(format!("target/{target}/release/lib{NAME}_ffi.a")));
    }

    let work = root.join("target/staging/xcframework");
    fresh_dir(&work)?;
    // Headers in a directory named after the module, so that another XCFramework in the same app
    // does not collide with this one's module.modulemap.
    let headers = work.join(format!("include/{PRODUCT}FFI"));
    let include = root.join(format!("crates/{NAME}-ffi/include"));
    for file in [format!("{NAME}.h"), "module.modulemap".to_owned()] {
        copy(&include.join(&file), &headers.join(&file))?;
    }
    let library = work.join(format!("lib{NAME}_ffi.a"));
    run(Command::new("lipo").arg("-create").args(&libraries).arg("-output").arg(&library))?;

    let framework = root.join(format!("packages/swift/{PRODUCT}FFI.xcframework"));
    if framework.exists() {
        fs::remove_dir_all(&framework).map_err(at(framework.display()))?;
    }
    run(Command::new("xcodebuild")
        .arg("-create-xcframework")
        .arg("-library")
        .arg(&library)
        .arg("-headers")
        .arg(work.join("include"))
        .arg("-output")
        .arg(&framework))?;
    Ok(framework)
}

fn dist(dir: Option<&Path>) -> Result {
    let root = root();
    let version = version()?;
    let dest = dir_or_default(dir, &version)?;
    if dest.exists() && fs::read_dir(&dest).map_err(at(dest.display()))?.next().is_some() {
        return Err(format!("{} is not empty", dest.display()));
    }
    let out = root.join("target/staging/release");
    fresh_dir(&out)?;

    let module = wasm()?;
    let framework = xcframework()?;

    let npm = format!("{NAME}-v{version}-npm.tgz");
    let xcf = format!("{NAME}-v{version}-xcframework.zip");
    // pnpm writes a lockfile where it runs, so it packs a copy rather than dist.
    let staged = root.join("target/staging/npm");
    fresh_dir(&staged)?;
    copy(&module, &staged)?;
    let pack = root.join("target/staging/pack");
    fresh_dir(&pack)?;
    run(Command::new("pnpm").args(["pack", "--pack-destination"]).arg(&pack).current_dir(&staged))?;
    let packed = format!("{}-{version}.tgz", NPM.trim_start_matches('@').replace('/', "-"));
    copy(&pack.join(packed), &out.join(&npm))?;
    zip(&framework, &out.join(&xcf))?;

    check_npm(&out.join(&npm))?;
    check_swift(&out.join(&xcf))?;

    let sums = capture(Command::new("shasum").args(["-a", "256", &npm, &xcf]).current_dir(&out))?;
    write(&out.join("SHA256SUMS"), &format!("{sums}\n"))?;
    let sha = |name: &str| -> Result<String> {
        sums.lines()
            .find_map(|line| line.strip_suffix(name).map(|hash| hash.trim().to_owned()))
            .ok_or_else(|| format!("no SHA-256 for {name}"))
    };
    let checksum = capture(Command::new("swift").args(["package", "compute-checksum"]).arg(out.join(&xcf)))?;
    if checksum != sha(&xcf)? {
        return Err(format!("swift package compute-checksum {xcf} is {checksum}, not its SHA-256"));
    }
    let size = |name: &str| fs::metadata(out.join(name)).map(|m| m.len()).map_err(at(out.join(name).display()));
    let git = |args: &[&str]| capture(Command::new("git").args(args).current_dir(&root));

    let files = json!([
        {
            "name": npm,
            "product": "npm",
            "package": NPM,
            "size": size(&npm)?,
            "sha256": sha(&npm)?,
        },
        {
            "name": xcf,
            "product": "xcframework",
            "module": format!("{PRODUCT}FFI"),
            "targets": APPLE_TARGETS,
            "macos": MACOS,
            "header": format!("{NAME}.h"),
            "size": size(&xcf)?,
            "sha256": sha(&xcf)?,
            "swiftpm_checksum": checksum,
        },
    ]);
    let tools = json!({
        "rustc": capture(Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into())).arg("-V").current_dir(&root))?,
        "binaryen": BINARYEN,
        "xcode": capture(Command::new("xcodebuild").arg("-version"))?.lines().next().unwrap_or_default(),
        "node": capture(Command::new("node").arg("--version"))?,
        "pnpm": capture(Command::new("pnpm").arg("--version"))?,
    });
    let manifest = json!({
        "name": NAME,
        "version": version,
        "commit": git(&["rev-parse", "HEAD"])?,
        "dirty": !git(&["status", "--porcelain", "--untracked-files=no"])?.is_empty(),
        "files": files,
        "tools": tools,
    });
    let manifest = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    write(&out.join("release-manifest.json"), &format!("{manifest}\n"))?;

    fs::create_dir_all(&dest).map_err(at(dest.display()))?;
    for name in [npm.as_str(), &xcf, "SHA256SUMS", "release-manifest.json"] {
        copy(&out.join(name), &dest.join(name))?;
    }
    println!("{}", dest.display());
    Ok(())
}

fn check_npm(tgz: &Path) -> Result {
    let root = root();
    let consumer = root.join("target/staging/consumer/npm");
    fresh_dir(&consumer)?;
    write(&consumer.join("package.json"), "{ \"private\": true, \"type\": \"module\" }\n")?;
    run(Command::new("pnpm").args(["add", "--offline", "--ignore-scripts"]).arg(tgz).current_dir(&consumer))?;
    run(Command::new("node")
        .args(["--test", "packages/npm/test/"])
        .env(format!("{}_PACKAGE", NAME.to_uppercase()), &consumer)
        .current_dir(&root))
}

fn check_swift(zip: &Path) -> Result {
    let root = root();
    let consumer = root.join("target/staging/consumer/swift");
    fresh_dir(&consumer)?;
    // A path dependency takes its identity from the directory name, as a URL one from the
    // repository's.
    let package = consumer.join(format!("{NAME}.rs"));
    copy(&root.join("Package.swift"), &package.join("Package.swift"))?;
    for item in ["Sources", "Tests"] {
        copy(&root.join("packages/swift").join(item), &package.join("packages/swift").join(item))?;
    }
    run(Command::new("ditto").args(["-x", "-k"]).arg(zip).arg(package.join("packages/swift")))?;
    run(Command::new("swift")
        .arg("test")
        .arg("--package-path")
        .arg(&package)
        .env(format!("{}_CORPUS", NAME.to_uppercase()), root.join(CORPUS)))?;

    let app = consumer.join("app");
    write(
        &app.join("Package.swift"),
        &format!(
            "// swift-tools-version: 6.2\n\
             import PackageDescription\n\n\
             let package = Package(\n\
             \x20   name: \"App\",\n\
             \x20   platforms: [.macOS(.v26)],\n\
             \x20   dependencies: [.package(path: \"../{NAME}.rs\")],\n\
             \x20   targets: [.executableTarget(name: \"App\", dependencies: [.product(name: \"{PRODUCT}\", package: \"{NAME}.rs\")])]\n\
             )\n"
        ),
    )?;
    copy(&root.join("packages/swift/Tests/consumer.swift"), &app.join("Sources/App/main.swift"))?;
    run(Command::new("swift").arg("run").arg("--package-path").arg(&app))
}

const RELEASE_LINE: &str = "let release: (url: String, checksum: String)? = ";

fn pin(check: bool, dir: Option<&Path>) -> Result {
    let root = root();
    let version = version()?;
    let dir = dir_or_default(dir, &version)?;
    let path = dir.join("release-manifest.json");
    let manifest: Value =
        serde_json::from_slice(&fs::read(&path).map_err(at(path.display()))?).map_err(at(path.display()))?;
    if manifest["version"] != version.as_str() {
        return Err(format!("{} is not a set of version {version}", dir.display()));
    }
    let file = manifest["files"]
        .as_array()
        .and_then(|files| files.iter().find(|f| f["product"] == "xcframework"))
        .ok_or_else(|| format!("{} lists no xcframework", dir.display()))?;
    let (Some(name), Some(sha), Some(checksum)) =
        (file["name"].as_str(), file["sha256"].as_str(), file["swiftpm_checksum"].as_str())
    else {
        return Err(format!("{} has an incomplete xcframework entry", dir.display()));
    };
    if sha != checksum {
        return Err(format!("{name}: SwiftPM checksum {checksum} is not its SHA-256 {sha}"));
    }
    let repository = metadata_field("repository")?;
    let value = format!(
        "(url: {}, checksum: {})",
        Value::from(format!("{repository}/releases/download/v{version}/{name}")),
        Value::from(checksum)
    );

    let path = root.join("Package.swift");
    let swift = fs::read_to_string(&path).map_err(at(path.display()))?;
    let lines: Vec<&str> = swift.lines().filter(|line| line.starts_with(RELEASE_LINE)).collect();
    let [current] = lines.as_slice() else {
        return Err(format!("Package.swift must hold one line starting `{RELEASE_LINE}`"));
    };
    let wanted = format!("{RELEASE_LINE}{value}");
    if check {
        if *current != wanted {
            return Err(format!("Package.swift does not name {name} of {}: run `cargo xtask pin`", dir.display()));
        }
        return Ok(());
    }
    write(&path, &swift.replacen(current, &wanted, 1))?;
    println!("Package.swift names {name} {checksum}");
    Ok(())
}

fn dir_or_default(dir: Option<&Path>, version: &str) -> Result<PathBuf> {
    match dir {
        Some(dir) => std::path::absolute(dir).map_err(at(dir.display())),
        None => Ok(root().join(format!("dist/v{version}"))),
    }
}

fn zip(item: &Path, to: &Path) -> Result {
    let parent = item.parent().ok_or_else(|| format!("{} has no parent", item.display()))?;
    let name = item.file_name().ok_or_else(|| format!("{} has no name", item.display()))?;
    let mut entries = Vec::new();
    normalize(item, Path::new(name), &mut entries)?;
    entries.sort();
    let mut list = String::new();
    for entry in &entries {
        let entry = entry.to_str().ok_or_else(|| format!("{} is not UTF-8", entry.display()))?;
        if entry.contains('\n') {
            return Err(format!("{entry:?} cannot be listed for zip"));
        }
        list.push_str(entry);
        list.push('\n');
    }
    if to.exists() {
        fs::remove_file(to).map_err(at(to.display()))?;
    }
    let mut command = Command::new("zip");
    // ZIP stores local time: one zone keeps the fixed time one value.
    command.args(["-X", "-D", "-q", "-@"]).arg(to).current_dir(parent).env("TZ", "UTC");
    eprintln!("+ {}", show(&command));
    let mut child = command.stdin(Stdio::piped()).spawn().map_err(at("zip"))?;
    let mut stdin = child.stdin.take().ok_or("zip has no stdin")?;
    std::io::Write::write_all(&mut stdin, list.as_bytes()).map_err(at("zip"))?;
    drop(stdin);
    let status = child.wait().map_err(at("zip"))?;
    if status.success() { Ok(()) } else { Err(format!("zip failed: {status}")) }
}

fn normalize(path: &Path, relative: &Path, entries: &mut Vec<PathBuf>) -> Result {
    use std::os::unix::fs::PermissionsExt as _;
    // 2000-01-01T00:00:00Z: ZIP times start in 1980.
    const TIME: u64 = 946_684_800;
    let meta = fs::symlink_metadata(path).map_err(at(path.display()))?;
    if meta.is_dir() {
        for entry in fs::read_dir(path).map_err(at(path.display()))? {
            let entry = entry.map_err(at(path.display()))?;
            normalize(&entry.path(), &relative.join(entry.file_name()), entries)?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        return Err(format!("{} is neither a file nor a directory", path.display()));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).map_err(at(path.display()))?;
    fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(TIME)))
        .map_err(at(path.display()))?;
    entries.push(relative.to_path_buf());
    Ok(())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask sits in the workspace root").to_path_buf()
}

fn version() -> Result<String> {
    metadata_field("version")
}

fn metadata_field(key: &str) -> Result<String> {
    let text = capture(cargo().args(["metadata", "--format-version", "1", "--no-deps", "--locked"]))?;
    let metadata: Value = serde_json::from_str(&text).map_err(|e| format!("cargo metadata: {e}"))?;
    let core = format!("{NAME}-core");
    metadata["packages"]
        .as_array()
        .and_then(|packages| packages.iter().find(|p| p["name"] == core.as_str()))
        .and_then(|p| p[key].as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("cargo metadata has no {key} of {core}"))
}

fn cargo() -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root());
    command
}

fn run(command: &mut Command) -> Result {
    eprintln!("+ {}", show(command));
    let status = command.status().map_err(at(show(command)))?;
    if status.success() { Ok(()) } else { Err(format!("{} failed: {status}", show(command))) }
}

fn capture(command: &mut Command) -> Result<String> {
    let output = command.stderr(Stdio::inherit()).output().map_err(at(show(command)))?;
    if !output.status.success() {
        return Err(format!("{} failed: {}", show(command), output.status));
    }
    String::from_utf8(output.stdout).map(|s| s.trim().to_owned()).map_err(at(show(command)))
}

fn show(command: &Command) -> String {
    let mut words = vec![command.get_program().to_string_lossy()];
    words.extend(command.get_args().map(|arg| arg.to_string_lossy()));
    words.join(" ")
}

fn fresh_dir(path: &Path) -> Result {
    if path.exists() {
        fs::remove_dir_all(path).map_err(at(path.display()))?;
    }
    fs::create_dir_all(path).map_err(at(path.display()))
}

fn copy(from: &Path, to: &Path) -> Result {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(at(parent.display()))?;
    }
    if from.is_dir() {
        fs::create_dir_all(to).map_err(at(to.display()))?;
        for entry in fs::read_dir(from).map_err(at(from.display()))? {
            let entry = entry.map_err(at(from.display()))?;
            copy(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(drop).map_err(at(from.display()))
    }
}

fn write(path: &Path, text: &str) -> Result {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(at(parent.display()))?;
    }
    fs::write(path, text).map_err(at(path.display()))
}

fn at<E: Display>(what: impl Display) -> impl FnOnce(E) -> String {
    let what = what.to_string();
    move |error| format!("{what}: {error}")
}
