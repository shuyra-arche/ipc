fn main() {
    let script = std::env::var("DEP_SHUYRA_KERNEL_LINKER_SCRIPT").expect("Kernel linker contract");
    println!(
        "cargo::rustc-link-arg-bin=twine-test-platform=--defsym=KERNEL_LOAD_ADDRESS=0x80200000"
    );
    println!("cargo::rustc-link-arg-bin=twine-test-platform=-T{script}");
    println!("cargo::rerun-if-changed={script}");
}
