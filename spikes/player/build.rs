// Linka o libmpv dinamicamente. No Linux ele vem do pacote do sistema
// (libmpv-dev). No Windows, aponte MPV_LIB_DIR para a pasta com a
// biblioteca de importação do libmpv.
fn main() {
    println!("cargo:rerun-if-env-changed=MPV_LIB_DIR");
    if let Ok(dir) = std::env::var("MPV_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
    }
    println!("cargo:rustc-link-lib=dylib=mpv");
}
