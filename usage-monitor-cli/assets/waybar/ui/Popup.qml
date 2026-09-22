import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// The Plasma popup (main.qml + FullPopup.qml), as a standalone window.
//
// Waybar paints the panel entry itself, so the plasmoid's compact representation
// has no counterpart here: this window *is* the popup, opened by Waybar's
// `on-click`. Everything below the header is the Plasma layout unchanged.
QQC2.ApplicationWindow {
    id: root

    // Set by the launcher: when false the process exits with the window instead
    // of staying resident for the next click.
    property bool residentMode: true
    // Pinned popups survive losing focus (the Plasma widget's "Keep open").
    property bool keepOpen: false
    // The settings payload arrives asynchronously, so the stored pin state is
    // applied whenever it lands — reading it once at startup left the popup
    // unpinned (and then wrote that back over the user's setting).
    property bool keepOpenTouched: false
    readonly property bool storedKeepOpen: root.settings.keepOpen === true
    onStoredKeepOpenChanged: if (!root.keepOpenTouched) root.keepOpen = root.storedKeepOpen
    property bool settingsOpen: false

    readonly property var summary: JSON.parse(backend.summaryJson)
    readonly property var settings: JSON.parse(backend.settingsJson)
    readonly property var cost: JSON.parse(backend.costJson)
    readonly property string monitorIcon: backend.uiDir + "/images/usage-monitor.png"

    property alias ui: themePalette
    // Test hook for the QML smoke test (widgets/waybar/tests/test_popup_qml.py).
    property alias updateBanner: updateBannerItem

    ThemePalette {
        id: themePalette
        // The summary payload carries the theme as well, so the popup is styled
        // from the first (fast) refresh instead of waiting for the settings load.
        spec: root.summary.theme || root.settings.theme || ({})
    }

    // Kept in sync with DEFAULT_WIDTH/DEFAULT_HEIGHT in
    // usage_monitor_waybar_popup.py, which can override them with --width/--height.
    width: 420
    height: 520
    minimumWidth: 320
    minimumHeight: 320
    visible: false
    title: "Usage Monitor"

    // Frameless so the popup looks like a panel popup rather than an app window.
    // A transparent window colour is only requested when the user asked for
    // transparency: without a compositor it would paint the rounded corners
    // black, and most bars run composited only on Wayland/picom setups.
    flags: Qt.Window | Qt.FramelessWindowHint | Qt.NoDropShadowWindowHint
    color: root.ui.translucent ? "transparent" : root.ui.backgroundColor

    // Only a real toggle is persisted; the sync above must not write back.
    onKeepOpenChanged: if (root.keepOpenTouched) backend.saveStateKey("keepOpen", root.keepOpen ? "true" : "false")

    onActiveChanged: {
        // Click-outside closes the popup, exactly like the Plasma dialog's
        // hideOnWindowDeactivate — unless it is pinned or settings are open.
        if (!active && visible && !root.keepOpen && !root.settingsOpen) {
            root.hide()
        }
    }

    onVisibleChanged: {
        if (visible) {
            root.refreshOnOpen()
        } else if (!root.residentMode) {
            Qt.quit()
        }
    }

    // Summary only: the timer path. Settings (slow, ~30 helper calls) reload
    // on open and on demand, never on tick.
    function refresh() {
        backend.refresh()
    }

    // Summary + settings: open path, so a CLI upgrade made while the popup
    // was closed shows the update banner immediately.
    function refreshOnOpen() {
        backend.refresh()
        backend.loadSettings()
    }

    function pinKey(entry) {
        // Mirrors pin_key_for_entry in usage_monitor_waybar_data.py: `provider`
        // for the implicit default login, `provider/account` for named accounts.
        if (entry && entry.account) return entry.provider + "/" + entry.account
        return entry ? entry.provider : ""
    }

    function compactLabelPct() {
        // The helper precomputes the pinned headline over the enabled bar
        // windows; fall back to the local match for older cached payloads.
        if (root.summary.pinnedPercent !== undefined && root.summary.pinnedPercent !== null) {
            return root.summary.pinnedPercent
        }
        var pinned = root.summary.barProvider || ""
        if (pinned) {
            var providers = root.summary.providers || []
            for (var i = 0; i < providers.length; i++) {
                if (pinKey(providers[i]) === pinned) {
                    return providers[i].maxPercent || 0
                }
            }
            // Legacy provider-level pin: first entry of that provider.
            if (pinned.indexOf("/") === -1) {
                for (var j = 0; j < providers.length; j++) {
                    if (providers[j].provider === pinned) {
                        return providers[j].maxPercent || 0
                    }
                }
            }
        }
        return root.summary.percentage || 0
    }

    function openSettings() {
        settingsLoader.active = true
        if (settingsLoader.item) {
            settingsLoader.item.show()
            settingsLoader.item.raise()
            settingsLoader.item.requestActivate()
        }
    }

    Component.onCompleted: {
        root.keepOpen = (root.settings.keepOpen === true)
        // Turns this window into an anchored Wayland layer surface (below the
        // bar) when the session supports it; a no-op on X11, where the launcher
        // positions the window itself. Must run before the window is shown.
        backend.applyLayerShell(root, "popup")
    }

    Connections {
        target: backend
        function onSettingsRequested() {
            root.openSettings()
        }
    }

    Timer {
        interval: Math.max(10, root.settings.refreshIntervalSeconds || 30) * 1000
        repeat: true
        running: root.visible
        triggeredOnStart: false
        onTriggered: root.refresh()
    }

    Loader {
        id: settingsLoader
        active: false
        source: "SettingsWindow.qml"
        onLoaded: {
            item.ui = Qt.binding(function() { return root.ui })
            item.closed.connect(function() { root.settingsOpen = false })
            item.opened.connect(function() { root.settingsOpen = true })
        }
    }

    // Background + outline, painted by the popup itself because the window is
    // frameless (in Plasma this was the dialog's job).
    background: Item {
        Rectangle {
            anchors.fill: parent
            color: root.ui.backgroundColor
            opacity: root.ui.backgroundOpacity
            radius: root.ui.cornerRadius
        }

        // Drawn at full opacity: at high transparency the fill alone is nearly
        // invisible and the popup reads as "nothing is there".
        Rectangle {
            anchors.fill: parent
            color: "transparent"
            radius: root.ui.cornerRadius
            border.width: 1
            border.color: root.ui.borderColor
        }
    }

    // Escape closes the popup, like any panel popup.
    Shortcut {
        sequences: [StandardKey.Cancel]
        onActivated: root.hide()
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: root.ui.largeSpacing
        spacing: root.ui.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
            spacing: root.ui.smallSpacing

            Image {
                source: root.monitorIcon
                sourceSize.width: Math.round(root.ui.gridUnit * 1.4)
                sourceSize.height: Math.round(root.ui.gridUnit * 1.4)
                Layout.preferredWidth: Math.round(root.ui.gridUnit * 1.4)
                Layout.preferredHeight: Math.round(root.ui.gridUnit * 1.4)
                Layout.alignment: Qt.AlignVCenter
                fillMode: Image.PreserveAspectFit
            }

            ColumnLayout {
                Layout.alignment: Qt.AlignVCenter
                Layout.fillWidth: true
                spacing: 0

                QQC2.Label {
                    text: "Usage Monitor"
                    font.bold: true
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.headingFontSize
                    color: root.ui.textColor
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    text: (root.summary.text || "--")
                          + (root.summary.class === "stale" ? " · cached/stale" : "")
                    opacity: 0.72
                    elide: Text.ElideRight
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.smallFontSize
                    color: root.ui.subtextColor
                }
            }

            QQC2.BusyIndicator {
                running: backend.busy
                visible: true
                opacity: backend.busy ? 1 : 0
                Layout.preferredWidth: root.ui.gridUnit
                Layout.preferredHeight: root.ui.gridUnit
                Layout.alignment: Qt.AlignVCenter
            }

            ThemedToolButton {
                ui: root.ui
                iconName: "refresh"
                text: "Refresh"
                tooltipText: "Refresh usage data"
                Layout.alignment: Qt.AlignVCenter
                // Explicit user action: bypasses the shared fetch interval.
                onClicked: backend.forceRefresh()
            }

            ThemedToolButton {
                ui: root.ui
                iconName: "chart"
                text: "Cost"
                tooltipText: "Fetch cost / spend data"
                Layout.alignment: Qt.AlignVCenter
                onClicked: backend.fetchCost()
            }

            ThemedToolButton {
                ui: root.ui
                iconName: "pin"
                text: "Keep open"
                checkable: true
                checked: root.keepOpen
                tooltipText: checked ? "Unpin popup (auto-hide)" : "Pin popup (keep open)"
                Layout.alignment: Qt.AlignVCenter
                onToggled: {
                    root.keepOpenTouched = true
                    root.keepOpen = checked
                }
            }

            ThemedToolButton {
                ui: root.ui
                iconName: "settings"
                text: "Settings"
                tooltipText: "Open settings"
                Layout.alignment: Qt.AlignVCenter
                onClicked: root.openSettings()
            }

            ThemedToolButton {
                ui: root.ui
                iconName: "close"
                text: "Close"
                tooltipText: root.residentMode ? "Close the popup" : "Quit"
                Layout.alignment: Qt.AlignVCenter
                onClicked: root.hide()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: root.ui.borderColor
            opacity: 0.6
        }

        UpdateBanner {
            id: updateBannerItem
            Layout.fillWidth: true
        }

        QQC2.Label {
            visible: backend.errorText.length > 0
            Layout.fillWidth: true
            text: backend.errorText
            wrapMode: Text.WordWrap
            font.family: root.ui.fontFamily
            font.pointSize: root.ui.fontSize
            color: root.ui.errorColor
        }

        ColumnLayout {
            visible: backend.errorDetails.length > 0
            Layout.fillWidth: true
            spacing: root.ui.smallSpacing

            RowLayout {
                Layout.fillWidth: true

                QQC2.Label {
                    Layout.fillWidth: true
                    text: "Error details"
                    font.bold: true
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.fontSize
                    color: root.ui.textColor
                }

                ThemedToolButton {
                    ui: root.ui
                    iconName: "copy"
                    text: "Copy"
                    tooltipText: "Copy the error details"
                    onClicked: backend.copyToClipboard(backend.errorDetails)
                }
            }

            QQC2.ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: root.ui.gridUnit * 6
                clip: true
                background: null

                QQC2.TextArea {
                    id: errorDetailsArea
                    text: backend.errorDetails
                    readOnly: true
                    selectByMouse: true
                    wrapMode: TextEdit.NoWrap
                    color: root.ui.textColor
                    font.family: "monospace"
                    background: null
                }
            }
        }

        UsagePage {
            Layout.fillWidth: true
            Layout.fillHeight: true
            ui: root.ui
            summary: root.summary
            settings: root.settings
            cost: root.cost
        }
    }
}
