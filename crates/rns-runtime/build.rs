fn main() {
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=web/app.js");
    println!("cargo:rerun-if-changed=web/style.css");
}
