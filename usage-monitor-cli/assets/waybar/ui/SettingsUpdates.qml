import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Updates page: installed vs CLI versions, one-click reinstall and release
// notes. Unlike the popup banner, this page always shows a pending update —
// dismissing the banner only hides the popup notice, never this page.
QQC2.ScrollView {
    id: page

    // SettingsWindow instance.
    property var host: null
    readonly property var ui: page.host ? page.host.ui : null
    readonly property var settings: page.host ? page.host.settings : ({})
    readonly property var update: page.settings.update || ({})

    function safeParse(raw, fallback) {
        try {
            var parsed = JSON.parse(raw || "")
            return parsed !== undefined ? parsed : fallback
        } catch (e) {
            return fallback
        }
    }

    readonly property var notes: page.safeParse(backend.updateNotesJson, ({"version": ""}))

    // Fetch the notes once the versions are known (settings arrive async).
    function maybeFetchNotes() {
        var u = page.update || ({})
        if (u.outdated === true && u.available && page.notes.version !== u.available) {
            backend.updateChangelog(u.available)
        }
    }

    Component.onCompleted: page.maybeFetchNotes()

    Connections {
        target: backend
        function onSettingsJsonChanged() {
            page.maybeFetchNotes()
        }
    }

    clip: true
    background: null

    function sectionMargins() {
        return page.ui ? page.ui.largeSpacing : 8
    }

    ColumnLayout {
        width: page.availableWidth
        spacing: page.ui ? page.ui.largeSpacing : 8

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: page.sectionMargins()
            spacing: page.ui ? page.ui.largeSpacing : 8

            GlyphIcon {
                name: "update"
                color: page.ui ? page.ui.textColor : "#f5f5f7"
                Layout.preferredWidth: page.ui ? page.ui.gridUnit * 2 : 32
                Layout.preferredHeight: page.ui ? page.ui.gridUnit * 2 : 32
                Layout.alignment: Qt.AlignVCenter
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                QQC2.Label {
                    Layout.fillWidth: true
                    text: "Usage Monitor popup " + (page.settings.popupVersion || "?")
                        + " · CLI " + (page.settings.cliVersion || "?")
                    wrapMode: Text.WordWrap
                    font.pointSize: page.ui ? page.ui.fontSize : 11
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    text: {
                        if (!page.update.outdated) {
                            return "You're up to date."
                        }
                        if (page.update.dismissed) {
                            return "Update " + (page.update.available || "?") + " available — popup notice dismissed."
                        }
                        return "Update " + (page.update.available || "?") + " available (installed " + (page.update.installed || "?") + ")."
                    }
                    wrapMode: Text.WordWrap
                    opacity: 0.75
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.subtextColor : "#98989d"
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: page.sectionMargins()
            Layout.rightMargin: page.sectionMargins()
            Layout.preferredHeight: 1
            color: page.ui ? page.ui.borderColor : "#48484a"
            opacity: 0.35
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: page.sectionMargins()
            Layout.rightMargin: page.sectionMargins()
            spacing: page.ui ? page.ui.smallSpacing : 4
            visible: page.update.outdated === true

            QQC2.Label {
                Layout.fillWidth: true
                text: "The update reinstalls the widget from the current CLI binary, the same as running "
                    + "`usage-monitor-cli widget install waybar`."
                wrapMode: Text.WordWrap
                opacity: 0.75
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: page.ui ? page.ui.smallSpacing : 4

                QQC2.Button {
                    text: "Update now"
                    enabled: !backend.busy
                    onClicked: backend.applyUpdate()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: "Reinstall the widget from the current CLI"
                    QQC2.ToolTip.delay: 500
                }

                QQC2.Button {
                    text: "Release notes"
                    visible: (page.update.url || "").length > 0
                    onClicked: Qt.openUrlExternally(page.update.url)
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: "Open the release page in a browser"
                    QQC2.ToolTip.delay: 500
                }

                Item { Layout.fillWidth: true }
            }

            QQC2.Label {
                Layout.fillWidth: true
                visible: backend.busy
                text: "Working…"
                opacity: 0.7
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }

            // Release notes, fetched on open and rendered from sanitized
            // markdown (Text.MarkdownText renders, never executes).
            QQC2.Label {
                Layout.fillWidth: true
                text: "What's new in " + (page.notes.version || page.update.available || "")
                font.bold: true
                font.family: page.ui ? page.ui.fontFamily : ""
                font.pointSize: page.ui ? page.ui.fontSize : 11
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            QQC2.ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: (page.ui ? page.ui.gridUnit : 16) * 12
                clip: true
                background: null

                QQC2.TextArea {
                    text: page.notes.body && page.notes.body.length > 0
                        ? page.notes.body
                        : "Loading release notes…"
                    textFormat: Text.MarkdownText
                    readOnly: true
                    selectByMouse: true
                    wrapMode: TextEdit.Wrap
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                    background: null
                }
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: page.notes.source === "github" ? "Source: GitHub Releases" : (page.notes.source === "release-file" ? "Source: GitHub release notes" : (page.notes.source === "embedded" ? "Source: bundled changelog (offline)" : ""))
                opacity: 0.6
                elide: Text.ElideRight
                font.family: page.ui ? page.ui.fontFamily : ""
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: page.sectionMargins()
            Layout.rightMargin: page.sectionMargins()
            spacing: page.ui ? page.ui.smallSpacing : 4
            visible: page.update.outdated !== true

            QQC2.Button {
                text: "Check again"
                enabled: !backend.busy
                onClicked: backend.loadSettings()
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Reload versions from the helper"
                QQC2.ToolTip.delay: 500
            }
        }

        Item { Layout.fillHeight: true }
    }
}
