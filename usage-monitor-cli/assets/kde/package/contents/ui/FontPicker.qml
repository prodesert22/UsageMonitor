import QtQuick
import QtQuick.Dialogs

// Font chooser, kept in its own file for two reasons:
//
// 1. Listing every installed family in a QQC2.ComboBox freezes Plasma: the
//    desktop style measures each entry eagerly, so a ~2000-family model burns
//    seconds of GUI-thread time (the whole shell locks up while the settings
//    page builds). The native dialog enumerates lazily instead.
// 2. The QtQuick.Dialogs import is isolated here, so a Plasma install without
//    the module only loses the "Choose…" button instead of breaking the page.
FontDialog {
    id: dialog

    signal picked(string family)

    title: "Choose a font"

    function openWith(family) {
        if (family) {
            dialog.selectedFont = Qt.font({ "family": family })
        }
        dialog.open()
    }

    onAccepted: dialog.picked(dialog.selectedFont.family)
}
