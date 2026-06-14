import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kcmutils as KCM
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

KCM.SimpleKCM {
    id: page

    // Plasma enables the Apply button when this is emitted, and calls saveConfig()
    // on Apply/OK. Display preferences stay pending until then.
    signal configurationChanged

    property var pending: ({})

    leftPadding: Kirigami.Units.gridUnit * 2
    rightPadding: Kirigami.Units.gridUnit * 2
    topPadding: Kirigami.Units.gridUnit
    bottomPadding: Kirigami.Units.gridUnit

    function setPending(key, value) {
        var p = page.pending
        p[key] = value
        page.pending = p
        page.configurationChanged()
    }

    function curr(key, fallback) {
        return (key in page.pending) ? page.pending[key] : fallback
    }

    function saveConfig() {
        backend.batchSetState(page.pending)
        // Optimistically reflect saved values so the controls don't flash back to
        // the previous (still-cached) settings before the helper reloads.
        var s = backend.settings
        for (var k in page.pending) {
            var v = page.pending[k]
            if (k === "showBarText" || k === "showAccountEmail")
                s[k] = (v === true || v === "true")
            else if (k === "refreshIntervalSeconds")
                s[k] = parseInt(v)
            else if (k === "barProvider")
                s["pinnedProvider"] = v
        }
        backend.settings = s
        page.pending = ({})
    }

    SettingsBackend {
        id: backend
    }

    ColumnLayout {
        width: parent.width
        spacing: Kirigami.Units.largeSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            ColumnLayout {
                spacing: 0

                PlasmaComponents3.Label {
                    text: "Refresh every"
                    opacity: 0.8
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                QQC2.SpinBox {
                    from: 10
                    to: 600
                    stepSize: 10
                    editable: true
                    value: page.curr("refreshIntervalSeconds", backend.settings.refreshIntervalSeconds || 30)
                    onValueModified: page.setPending("refreshIntervalSeconds", value)
                }
            }

            PlasmaComponents3.Label {
                text: "seconds"
                Layout.alignment: Qt.AlignBottom
                opacity: 0.7
                bottomPadding: Kirigami.Units.smallSpacing
            }

            Item { Layout.fillWidth: true }
        }

        Flow {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            ColumnLayout {
                spacing: 0

                PlasmaComponents3.Label {
                    text: "Show bar text"
                    opacity: 0.8
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                QQC2.Switch {
                    id: showBarTextSwitch
                    checked: page.curr("showBarText", backend.settings.showBarText !== false)
                    onToggled: page.setPending("showBarText", checked)

                    QQC2.ToolTip.visible: showBarTextSwitch.hovered
                    QQC2.ToolTip.text: "Show usage percentage next to the icon in the panel bar."
                    QQC2.ToolTip.delay: 500
                }
            }

            ColumnLayout {
                spacing: 0

                PlasmaComponents3.Label {
                    text: "Show account email"
                    opacity: 0.8
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                QQC2.Switch {
                    id: showAccountEmailSwitch
                    checked: page.curr("showAccountEmail", backend.settings.showAccountEmail !== false)
                    onToggled: page.setPending("showAccountEmail", checked)

                    QQC2.ToolTip.visible: showAccountEmailSwitch.hovered
                    QQC2.ToolTip.text: "Show the account label below each provider in the usage view."
                    QQC2.ToolTip.delay: 500
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Kirigami.Theme.disabledTextColor
            opacity: 0.18
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                PlasmaComponents3.Label {
                    text: "Pin to panel bar"
                    opacity: 0.8
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                QQC2.ComboBox {
                    Layout.fillWidth: true
                    model: ["— none —"].concat((backend.settings.pinnableProviders || []).map(function(p) { return p.displayName || p.id }))
                    currentIndex: {
                        var pinned = page.curr("barProvider", backend.settings.pinnedProvider || "")
                        if (!pinned) return 0
                        var list = backend.settings.pinnableProviders || []
                        for (var i = 0; i < list.length; i++) {
                            if (list[i].id === pinned) return i + 1
                        }
                        return 0
                    }
                    onActivated: {
                        if (index === 0) {
                            page.setPending("barProvider", "")
                        } else {
                            var list = backend.settings.pinnableProviders || []
                            var picked = list[index - 1]
                            page.setPending("barProvider", picked.id || "")
                        }
                    }
                }
            }

            ColumnLayout {
                Layout.alignment: Qt.AlignBottom

                QQC2.ToolButton {
                    text: "Clear Cache"
                    icon.name: "edit-clear-history"
                    display: QQC2.AbstractButton.TextBesideIcon
                    onClicked: backend.cacheClear()

                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: "Clears the widget's last-good cache. Refresh after clearing."
                    QQC2.ToolTip.delay: 500
                }
            }
        }

        Item { Layout.fillHeight: true }

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.largeSpacing
            text: "Usage Monitor KDE " + (backend.settings.plasmoidVersion || "?") + " · CLI " + (backend.settings.cliVersion || "?")
            horizontalAlignment: Text.AlignHCenter
            opacity: 0.55
            font.pointSize: Kirigami.Theme.smallFont.pointSize
            elide: Text.ElideRight
        }
    }
}
