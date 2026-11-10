fn main() {
    let root = env!("CARGO_MANIFEST_DIR");
    println!("cargo:rustc-link-arg=-T{root}/link.ld");
    println!("cargo:rerun-if-changed=link.ld");
}
