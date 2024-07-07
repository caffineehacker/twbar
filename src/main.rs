use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow};
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, Layer, LayerShell};

fn activate(app: &Application) {
    let window = ApplicationWindow::new(app);

    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.auto_exclusive_zone_enable();

    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, false);

    let label = gtk::Label::new(Some("Test label"));
    window.set_child(Some(&label));
    window.show()
}

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id("com.timwaterhouse.twbar")
        .build();

    app.connect_activate(|app| activate(app));

    app.run()
}
