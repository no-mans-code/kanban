// The frontend is embedded with rust-embed. Cargo doesn't know about those
// files, so without this a rebuilt frontend would not reach the binary.
fn main() {
    println!("cargo:rerun-if-changed=../web/dist");
}
