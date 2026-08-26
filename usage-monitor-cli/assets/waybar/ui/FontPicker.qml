import QtQuick
import QtQuick.Dialogs

// Font chooser, kept in its own file so the QtQuick.Dialogs import is isolated:
// several distributions ship Qt Quick without the Dialogs module, and there the
// Loader in SettingsTheme.qml simply reports Error and hides the "Choose…"
// button instead of breaking the whole page.
//
// It is also the reason a ComboBox is not used for fonts: listing every
// installed family measures each entry eagerly and stalls the UI.
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
