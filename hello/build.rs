fn main() {
    println!("cargo:rustc-link-arg=--image-base=0x5000000");
    // println!("cargo:rustc-link-arg=-Trodata-segment=5100000");
}