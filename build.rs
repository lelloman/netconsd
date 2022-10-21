use std::process::Command;

fn main() {
    Command::new("make")
        .arg("clean")
        .arg("rlib")
        .status()
        .expect("make build failed");

    println!("cargo:rustc-link-search=native=./");
    println!("cargo:rustc-link-lib=static=netconsd");
}
