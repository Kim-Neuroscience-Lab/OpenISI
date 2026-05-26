fn main() {
    let attrs = tauri_attributes();

    if let Err(error) = tauri_build::try_build(attrs) {
        let error = format!("{error:#}");
        println!("{error}");
        if error.starts_with("unknown field") {
            print!(
                "found an unknown configuration field. This usually means that you are using a CLI version that is newer than `tauri-build` and is incompatible. "
            );
            println!(
                "Please try updating the Rust crates by running `cargo update` in the Tauri app folder."
            );
        }
        std::process::exit(1);
    }

    // Bake the vendored libtorch path into every binary's RPATH so the
    // produced executables can locate `@rpath/libtorch*.dylib` (macOS) or
    // `libtorch*.so` (Linux) without DYLD_LIBRARY_PATH / LD_LIBRARY_PATH
    // being set in the caller's shell. The .cargo/config.toml env vars are
    // a build-time hint to tch's build script and a dev-time convenience
    // for cargo-launched processes; they must not be load-bearing for
    // direct binary invocation.
    emit_libtorch_rpath();

    // Unit/integration test executables do not get the Windows manifest from tauri-winres
    // (`rustc-link-arg-bins` only). Without Common Controls v6, loading fails with
    // STATUS_ENTRYPOINT_NOT_FOUND on MSVC — see https://github.com/tauri-apps/tauri/issues/13419
    #[cfg(windows)]
    link_common_controls_manifest_for_all_targets();
}

/// Emit a per-binary `-rpath,<libtorch>/lib` linker arg on macOS and Linux
/// so produced binaries can locate the vendored libtorch at runtime.
/// Windows uses DLL search semantics and is handled separately (DLLs are
/// expected to sit alongside the .exe; the setup script arranges this).
fn emit_libtorch_rpath() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        return;
    }

    // LIBTORCH is set by .cargo/config.toml to the resolved vendor/libtorch
    // path. If it isn't set, the developer's environment is misconfigured
    // and a binary produced here would dyld-fail at runtime; refuse to
    // build a silently-broken binary.
    let libtorch = std::env::var("LIBTORCH").unwrap_or_else(|_| {
        eprintln!(
            "build.rs: LIBTORCH not set. .cargo/config.toml should resolve it to the \
             vendored libtorch directory. Run scripts/setup.sh to populate vendor/libtorch."
        );
        std::process::exit(1);
    });
    let lib_dir = std::path::Path::new(&libtorch).join("lib");
    let lib_dir = lib_dir.canonicalize().unwrap_or_else(|e| {
        eprintln!(
            "build.rs: cannot resolve {}/lib: {e}. Run scripts/setup.sh to populate vendor/libtorch.",
            libtorch
        );
        std::process::exit(1);
    });

    println!("cargo:rerun-if-env-changed=LIBTORCH");
    println!("cargo:rerun-if-changed={}", lib_dir.display());

    // `rustc-link-arg-bins` applies to every `[[bin]]` target in this crate
    // (i.e. `openisi` and `headless`). The lib target is consumed by the
    // bins, not loaded directly, so it doesn't need its own rpath.
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{}", lib_dir.display());
}

fn tauri_attributes() -> tauri_build::Attributes {
    #[cfg(windows)]
    {
        tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        )
    }
    #[cfg(not(windows))]
    {
        tauri_build::Attributes::new()
    }
}

#[cfg(windows)]
fn link_common_controls_manifest_for_all_targets() {
    if std::env::var("CARGO_CFG_TARGET_ENV").ok().as_deref() != Some("msvc") {
        return;
    }

    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("manifest/windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
