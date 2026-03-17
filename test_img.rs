fn main() {
    let path = "/home/codegoddy/.config/background";
    match image::open(path) {
        Ok(img) => println!("Success: {:?} {:?}", img.dimensions(), img.color()),
        Err(e) => println!("Error: {:?}", e),
    }
}
