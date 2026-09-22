import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Self-update banner: shown when the installed popup is older than the CLI
// binary. "What's new" fetches the release notes (GitHub Releases, cached),
// "Update now" reinstalls the widget the same way `widget install` does.
ColumnLayout {
    id: banner

    readonly property var update: (root.settings && root.settings.update) || ({})

    // Backend payloads arrive as JSON strings; a malformed one must hide the
    // section, not throw inside a binding.
    function safeParse(raw, fallback) {
        try {
            var parsed = JSON.parse(raw || "")
            return parsed !== undefined ? parsed : fallback
        } catch (e) {
            return fallback
        }
    }

    readonly property var notes: banner.safeParse(backend.updateNotesJson, ({"version": ""}))
    readonly property var updateResult: banner.safeParse(backend.updateResultJson, ({"status": ""}))
    readonly property string updateMessage: {
        var result = banner.updateResult || ({})
        if (!result.status) {
            return ""
        }
        if (result.status === "ok") {
            return "Widget updated — reloading…"
        }
        return "Update failed: " + (result.stderr || result.stdout || "")
    }

    property bool notesOpen: false
    property bool applying: false
    property string notesRequested: ""
    // Test hook: the headline the banner shows for the pending update.
    readonly property string bannerText: headline.text

    visible: banner.update.outdated === true && banner.update.dismissed !== true
    spacing: root.ui.smallSpacing

    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: bannerContent.implicitHeight + root.ui.smallSpacing * 2
        radius: root.ui.cornerRadius
        // Alpha on the colour, not `opacity` on the item: opacity would also
        // fade the text and buttons inside the banner.
        color: Qt.alpha(root.ui.warningColor, 0.16)
        border.width: 1
        border.color: Qt.alpha(root.ui.warningColor, 0.55)

        // Two stacked rows (headline, then actions): a single RowLayout let the
        // headline wrap word-by-word when the popup measured the banner before
        // its own width was known.
        ColumnLayout {
            id: bannerContent
            anchors.fill: parent
            anchors.margins: root.ui.smallSpacing
            spacing: root.ui.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: root.ui.smallSpacing

                GlyphIcon {
                    name: "update"
                    color: root.ui.textColor
                    Layout.preferredWidth: root.ui.gridUnit
                    Layout.preferredHeight: root.ui.gridUnit
                    Layout.alignment: Qt.AlignVCenter
                }

                QQC2.Label {
                    id: headline
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignVCenter
                    text: "Update " + (banner.update.available || "?") + " available (installed " + (banner.update.installed || "?") + ")"
                    elide: Text.ElideRight
                    maximumLineCount: 1
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.fontSize
                    color: root.ui.textColor
                }

                ThemedToolButton {
                    ui: root.ui
                    iconName: "close"
                    text: "Dismiss"
                    tooltipText: "Hide this update notice"
                    Layout.alignment: Qt.AlignVCenter
                    onClicked: backend.dismissUpdate(banner.update.available)
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: root.ui.smallSpacing

                Item { Layout.fillWidth: true }

                ThemedToolButton {
                    ui: root.ui
                    iconName: "info"
                    text: "What's new"
                    tooltipText: "Show the release notes"
                    Layout.alignment: Qt.AlignVCenter
                    onClicked: {
                        // Toggle; the panel opens when the matching notes
                        // arrive (see onUpdateNotesChanged below).
                        if (banner.notesOpen) {
                            banner.notesOpen = false
                            banner.notesRequested = ""
                            return
                        }
                        banner.notesRequested = banner.update.available
                        backend.updateChangelog(banner.update.available)
                    }
                }

                ThemedToolButton {
                    ui: root.ui
                    iconName: "update"
                    text: banner.applying ? "Updating…" : "Update now"
                    tooltipText: "Reinstall the widget from the current CLI"
                    enabled: !banner.applying && !backend.busy
                    Layout.alignment: Qt.AlignVCenter
                    onClicked: {
                        banner.applying = true
                        backend.applyUpdate()
                    }
                }
            }
        }
    }

    QQC2.Label {
        visible: (banner.applying && backend.busy) || banner.updateMessage.length > 0
        Layout.fillWidth: true
        text: banner.updateMessage.length > 0 ? banner.updateMessage : "Updating widget…"
        wrapMode: Text.WordWrap
        opacity: 0.8
        font.family: root.ui.fontFamily
        font.pointSize: root.ui.smallFontSize
        color: root.ui.subtextColor
    }

    // Release notes, fetched on demand and shown inline.
    ColumnLayout {
        visible: banner.notesOpen
        Layout.fillWidth: true
        spacing: root.ui.smallSpacing

        QQC2.Label {
            Layout.fillWidth: true
            text: "What's new in " + (banner.notes.version || "")
            font.bold: true
            font.family: root.ui.fontFamily
            font.pointSize: root.ui.fontSize
            color: root.ui.textColor
        }

        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: root.ui.gridUnit * 10
            clip: true
            background: null

            QQC2.TextArea {
                text: banner.notes.body && banner.notes.body.length > 0
                    ? banner.notes.body
                    : "No notes available offline. See the release page instead."
                textFormat: Text.MarkdownText
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
                color: root.ui.textColor
                background: null
            }
        }

        RowLayout {
            Layout.fillWidth: true
            // Action first: at the right edge the button would sit in the
            // scrollbar column of the notes view above.
            ThemedToolButton {
                ui: root.ui
                visible: (banner.notes.url || "").length > 0
                iconName: "info"
                text: "Open release page"
                tooltipText: "Open the release page in a browser"
                onClicked: Qt.openUrlExternally(banner.notes.url)
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: banner.notes.source === "github" ? "Source: GitHub Releases" : (banner.notes.source === "release-file" ? "Source: GitHub release notes" : (banner.notes.source === "embedded" ? "Source: bundled changelog (offline)" : ""))
                opacity: 0.6
                elide: Text.ElideRight
                font.family: root.ui.fontFamily
                font.pointSize: root.ui.smallFontSize
                color: root.ui.subtextColor
            }
        }
    }

    Connections {
        target: backend
        function onSettingsJsonChanged() {
            banner.applying = false
        }
        // The notes panel opens on response (KDE parity), not on click, so a
        // failed fetch shows the release-page link instead of an empty panel.
        function onUpdateNotesChanged() {
            if (banner.notesRequested !== "" && banner.notes.version === banner.notesRequested) {
                banner.notesOpen = true
                banner.notesRequested = ""
            }
        }
        function onUpdateResultChanged() {
            banner.applying = false
        }
    }
}
