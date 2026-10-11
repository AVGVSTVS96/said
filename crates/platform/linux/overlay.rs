use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use said_core::Result;

pub fn configure(
    window: &impl IsA<gtk::Window>,
    layer_shell: bool,
    monitor: Option<&gtk::gdk::Monitor>,
) -> Result<()> {
    let window: &gtk::Window = window.as_ref();
    if window.is_visible() || window.is_realized() {
        if layer_shell && window.is_layer_window() {
            if let Some(monitor) = monitor {
                window.set_monitor(monitor);
            }
            return Ok(());
        }
        if !layer_shell {
            window.set_keep_above(true);
            return Ok(());
        }
        return Err("configure the overlay before its first realization".into());
    }
    if layer_shell && window.is_layer_window() {
        if let Some(monitor) = monitor {
            window.set_monitor(monitor);
        }
        return Ok(());
    }
    window.set_accept_focus(false);
    window.set_focus_on_map(false);
    window.set_decorated(false);
    window.set_resizable(false);
    let (width, height) = window.default_size();
    window.set_size_request(width, height);
    if layer_shell {
        if !gtk_layer_shell::is_supported() {
            return Err("Niri layer-shell is unavailable".into());
        }
        window.init_layer_shell();
        window.set_namespace("said-overlay");
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(0);
        window.set_anchor(Edge::Bottom, true);
        window.set_layer_shell_margin(Edge::Bottom, 24);
        if let Some(monitor) = monitor {
            window.set_monitor(monitor);
        }
    } else {
        window.set_keep_above(true);
    }
    Ok(())
}
