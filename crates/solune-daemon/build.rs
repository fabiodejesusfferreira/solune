// Linka o libmpv dinamicamente.
//
// No Linux, o link é direto com o libmpv.so.2, o mesmo arquivo que o
// processo carrega ao rodar. Assim basta o pacote de runtime (libmpv2 no
// Debian, Ubuntu e Mint), sem o -dev, que é quem cria o libmpv.so.
//
// No Windows, aponte MPV_LIB_DIR para a pasta com a biblioteca de
// importação do libmpv. Nos dois sistemas, MPV_LIB_DIR entra na busca.
fn main() {
    println!("cargo:rerun-if-env-changed=MPV_LIB_DIR");
    if let Ok(dir) = std::env::var("MPV_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=dylib:+verbatim=libmpv.so.2");
    } else {
        println!("cargo:rustc-link-lib=dylib=mpv");
    }
}
