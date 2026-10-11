import pathlib, sys
import gi
gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, GLib
GLib.set_prgname('said-gtk-scratch')
output = pathlib.Path(sys.argv[1])
output.write_bytes(b"")
window = Gtk.Window(title='said GTK scratch')
window.set_default_size(640, 400)
view = Gtk.TextView()
window.add(view)
view.get_buffer().connect('changed', lambda buffer: output.write_bytes(buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), True).encode()))
def key_event(window, event):
    with output.with_suffix('.keys').open('a') as log: log.write(str(event.keyval) + '\n')
    return False
window.connect('key-press-event', key_event)
window.connect('destroy', Gtk.main_quit)
window.show_all()
view.grab_focus()
output.with_suffix(".ready").touch()
Gtk.main()
