fn main() {
    gtk4::init().unwrap();
    println!("Supported: {}", gtk4_layer_shell::is_supported());
}
