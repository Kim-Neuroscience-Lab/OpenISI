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

    #[cfg(windows)]
    link_common_controls_manifest_for_all_targets();
}

fn tauri_attributes() -> tauri_build::Attributes {
    #[cfg(windows)]
    {
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest())
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
