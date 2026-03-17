
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box, CssProvider};

fn main() {
    let app = Application::builder().application_id("com.test").build();
    app.connect_activate(|app| {
        let provider = CssProvider::new();
        provider.load_from_data(
            ".test-win { background-color: transparent; }
             .test-root { background-color: red; border-radius: 12px; }
            "
        );
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().unwrap(),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
        let root = Box::new(gtk4::Orientation::Vertical, 0);
        root.add_css_class("test-root");
        root.set_margin_top(20);
        root.set_margin_bottom(20);
        root.set_margin_start(20);
        root.set_margin_end(20);
        
        let win = ApplicationWindow::builder()
            .application(app)
            .default_width(200)
            .default_height(200)
            .decorated(false)
            .build();
        win.add_css_class("test-win");
        win.set_child(Some(&root));
        win.present();
    });
    app.run_with_args::<String>(&[]);
}

