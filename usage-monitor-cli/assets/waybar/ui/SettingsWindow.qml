import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// The four Plasma config pages (General, Providers, Order, Theme) in one window.
//
// Plasma supplied the dialog, the page list and the Apply/Cancel buttons through
// KCM; outside Plasma that chrome has to exist here. The pending-edits contract
// is the same: pages call setPending(), Apply writes every key in one
// batch-set-state, Discard drops them.
QQC2.ApplicationWindow {
    id: settingsWindow

    // ThemePalette instance, bound by the popup that loads this window.
    property var ui: null

    readonly property var settings: JSON.parse(backend.settingsJson)
    readonly property var catalog: settingsWindow.settings.themeCatalog
                                   || ({ "builtin": [], "schemes": [], "custom": [] })
    property var pending: ({})
    readonly property bool dirty: Object.keys(settingsWindow.pending).length > 0

    signal opened()
    signal closed()

    width: 640
    height: 680
    minimumWidth: 480
    minimumHeight: 420
    title: "Usage Monitor — Settings"
    color: settingsWindow.ui ? settingsWindow.ui.backgroundColor : "#1c1c1e"

    onVisibleChanged: visible ? settingsWindow.opened() : settingsWindow.closed()

    // Under layer-shell this window is a layer surface too; "settings" asks for
    // an unanchored (centred) one instead of the popup's bar-edge anchoring.
    Component.onCompleted: backend.applyLayerShell(settingsWindow, "settings")

    // Pending edits are applied by the footer buttons, never on the fly: a
    // half-typed hex colour or a mid-drag interval would otherwise be written to
    // state.json and re-rendered immediately.
    function setPending(key, value) {
        // Copy instead of mutating: reassigning the same object does not notify
        // the bindings that read settingsWindow.pending.
        var next = {}
        for (var k in settingsWindow.pending) {
            next[k] = settingsWindow.pending[k]
        }
        next[key] = value
        settingsWindow.pending = next
    }

    function curr(key, fallback) {
        return (key in settingsWindow.pending) ? settingsWindow.pending[key] : fallback
    }

    // Pending edit first, then the saved state key, then the built-in default.
    function stateValue(key, fallback) {
        if (key in settingsWindow.pending) {
            return String(settingsWindow.pending[key])
        }
        var saved = settingsWindow.settings.themeState || ({})
        var value = saved[key]
        return (value !== undefined && String(value) !== "") ? String(value) : fallback
    }

    function apply() {
        var pairs = []
        for (var key in settingsWindow.pending) {
            pairs.push([String(key), String(settingsWindow.pending[key])])
        }
        if (pairs.length > 0) {
            backend.batchSetState(JSON.stringify(pairs))
        }
        settingsWindow.pending = ({})
    }

    function discard() {
        settingsWindow.pending = ({})
    }

    header: QQC2.TabBar {
        id: tabBar
        currentIndex: 0

        QQC2.TabButton { text: "General" }
        QQC2.TabButton { text: "Providers" }
        QQC2.TabButton { text: "Order" }
        QQC2.TabButton { text: "Theme" }
    }

    footer: Item {
        implicitHeight: footerRow.implicitHeight + (settingsWindow.ui ? settingsWindow.ui.largeSpacing : 8) * 2

        RowLayout {
            id: footerRow
            anchors.fill: parent
            anchors.margins: settingsWindow.ui ? settingsWindow.ui.largeSpacing : 8
            spacing: settingsWindow.ui ? settingsWindow.ui.smallSpacing : 4

            QQC2.Label {
                Layout.fillWidth: true
                text: settingsWindow.dirty
                      ? "Unsaved changes"
                      : "Usage Monitor popup " + (settingsWindow.settings.popupVersion || "?")
                        + " · CLI " + (settingsWindow.settings.cliVersion || "?")
                opacity: 0.65
                elide: Text.ElideRight
                font.pointSize: settingsWindow.ui ? settingsWindow.ui.smallFontSize : 9
                color: settingsWindow.ui ? settingsWindow.ui.subtextColor : "#98989d"
            }

            QQC2.Button {
                text: "Discard"
                enabled: settingsWindow.dirty
                onClicked: settingsWindow.discard()
            }

            QQC2.Button {
                text: "Apply"
                enabled: settingsWindow.dirty
                onClicked: settingsWindow.apply()
            }

            QQC2.Button {
                text: "Close"
                onClicked: settingsWindow.close()
            }
        }
    }

    StackLayout {
        anchors.fill: parent
        currentIndex: tabBar.currentIndex

        SettingsGeneral { host: settingsWindow }
        SettingsProviders { host: settingsWindow }
        SettingsOrder { host: settingsWindow }
        SettingsTheme { host: settingsWindow }
    }
}
