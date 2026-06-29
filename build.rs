fn main() {
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libpeinit.so.0");
    println!("cargo:rerun-if-changed=build.rs");
}
