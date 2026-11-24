use std::{env, fs, path::PathBuf};

fn main() -> std::io::Result<()> {
    if env::var("TARGET").as_deref() != Ok("wasm32-unknown-unknown") {
        return Ok(());
    }

    // Embed the same bundled files that the native asset loader searches.
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/classic");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut assets = String::from("&[\n");
    for folder in ["graphics", "sound"] {
        let mut entries = fs::read_dir(root.join(folder))?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if entry.file_type()?.is_file() {
                let name = entry.file_name();
                let key = format!("{folder}/{}", name.to_string_lossy().to_ascii_lowercase());
                assets.push_str(&format!(
                    "({key:?}, include_bytes!({:?})),\n",
                    entry.path().canonicalize()?
                ));
            }
        }
    }
    assets.push_str("]\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("classic_assets.rs"),
        assets,
    )
}
