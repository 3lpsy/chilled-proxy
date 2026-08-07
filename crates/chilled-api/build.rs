// include_dir!(../../dist) needs the directory to exist even in UI-less dev
// builds; the binary then serves a clear 503 instead of failing to compile.
fn main() -> std::io::Result<()> {
    std::fs::create_dir_all("../../dist")?;
    println!("cargo:rerun-if-changed=../../dist");
    Ok(())
}
