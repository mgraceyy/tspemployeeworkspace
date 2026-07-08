// Patches are applied manually via patches/apply-all.ps1 (or fix-permissions-and-apply.ps1).
// Set DTR_APPLY_PATCHES=1 at compile time only if you still need automatic copy from patches/replacements/.
use std::env;

fn main() {
    if env::var("DTR_APPLY_PATCHES")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        apply_patches();
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DTR_APPLY_PATCHES");
}

fn apply_patches() {
    use std::fs;
    use std::path::Path;

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let patch_root = Path::new(&manifest_dir).join("patches/replacements");

    let mappings = [
        ("src_models_settings.rs", "src/models/settings.rs"),
        ("src_services_leave.rs", "src/services/leave.rs"),
    ];

    for (src_name, dest_rel) in mappings {
        let src = patch_root.join(src_name);
        let dest = Path::new(&manifest_dir).join(dest_rel);
        if src.exists() {
            if let Err(err) = fs::copy(&src, &dest) {
                println!("cargo:warning=patch copy failed for {dest_rel}: {err}");
            } else {
                println!("cargo:warning=applied patch {dest_rel}");
            }
            println!("cargo:rerun-if-changed={}", src.display());
        }
    }
}
