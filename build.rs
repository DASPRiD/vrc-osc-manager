use image::Rgba;
use std::{env, fs, path::Path};
extern crate embed_resource;

const INACTIVE_PNG_ICON: &[u8] = include_bytes!("assets/icon-inactive.png");
const ACTIVE_PNG_ICON: &[u8] = include_bytes!("assets/icon-active.png");

fn convert(img: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    let img = image::load_from_memory(img)?;
    let mut img = img.to_rgba8();

    for Rgba(pixel) in img.pixels_mut() {
        *pixel = u32::from_be_bytes(*pixel).rotate_right(8).to_be_bytes();
    }

    Ok(img.into_raw())
}

fn main() {
    if env::var_os("CARGO_CFG_TARGET_OS").unwrap() == "linux" {
        let out_dir = &env::var_os("OUT_DIR").unwrap();
        let out_path = Path::new(out_dir);

        fs::write(
            out_path.join("linux-inactive-icon"),
            convert(INACTIVE_PNG_ICON).unwrap(),
        )
        .unwrap();
        fs::write(
            out_path.join("linux-active-icon"),
            convert(ACTIVE_PNG_ICON).unwrap(),
        )
        .unwrap();
    }

    if env::var_os("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        embed_resource::compile("assets/icons.rc", embed_resource::NONE);
    }

    println!("cargo:rerun-if-changed=assets/icons.rc");
    println!("cargo:rerun-if-changed=assets/icon-inactive.ico");
    println!("cargo:rerun-if-changed=assets/icon-active.ico");
    println!("cargo:rerun-if-changed=build.rs");
}
