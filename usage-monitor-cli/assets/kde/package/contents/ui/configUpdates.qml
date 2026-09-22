import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kcmutils as KCM
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

// Updates page: installed vs CLI versions, one-click reinstall and release
// notes. Unlike the popup banner, this page always shows a pending update —
// dismissing the banner only hides the popup notice, never this page.
KCM.SimpleKCM {
    id: page

    signal configurationChanged

    readonly property var update: (backend.settings && backend.settings.update) || ({})

    leftPadding: Kirigami.Units.gridUnit * 2
    rightPadding: Kirigami.Units.gridUnit * 2
    topPadding: Kirigami.Units.gridUnit
    bottomPadding: Kirigami.Units.gridUnit

    // No pending edits on this page; the buttons act immediately.
    function saveConfig() {
    }

    // Fetch the notes once the versions are known (settings arrive async).
    function maybeFetchNotes() {
        var u = page.update || ({})
        if (u.outdated === true && backend.updateNotes.version !== u.available) {
            backend.updateShowChangelog()
        }
    }

    Component.onCompleted: page.maybeFetchNotes()

    Connections {
        target: backend
        function onSettingsChanged() {
            page.maybeFetchNotes()
        }
    }

    SettingsBackend {
        id: backend
    }

    ColumnLayout {
        width: parent.width
        spacing: Kirigami.Units.largeSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                source: "system-software-update"
                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                Layout.alignment: Qt.AlignVCenter
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: "Usage Monitor KDE " + (backend.settings.plasmoidVersion || "?")
                        + " · CLI " + (backend.settings.cliVersion || "?")
                    wrapMode: Text.WordWrap
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize
                }

                PlasmaComponents3.Label {
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
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                    color: Kirigami.Theme.disabledTextColor
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Kirigami.Theme.disabledTextColor
            opacity: 0.18
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            visible: page.update.outdated === true

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: "The update reinstalls the widget from the current CLI binary, the same as running "
                    + "`usage-monitor-cli widget install kde`. If the new version brings interface changes, "
                    + "restart Plasma (or log out and back in) to load them."
                wrapMode: Text.WordWrap
                opacity: 0.75
                font.pointSize: Kirigami.Theme.smallFont.pointSize
                color: Kirigami.Theme.disabledTextColor
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                QQC2.Button {
                    text: "Update now"
                    icon.name: "system-software-update"
                    enabled: !backend.busy
                    onClicked: backend.updateApply()
                }

                QQC2.Button {
                    text: "Release notes"
                    icon.name: "internet-web-browser"
                    visible: (page.update.url || "").length > 0
                    onClicked: Qt.openUrlExternally(page.update.url)

                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: "Open the release page in a browser"
                    QQC2.ToolTip.delay: 500
                }

                Item { Layout.fillWidth: true }
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                visible: backend.busy
                text: "Working…"
                opacity: 0.7
                font.pointSize: Kirigami.Theme.smallFont.pointSize
                color: Kirigami.Theme.disabledTextColor
            }

            // Release notes, fetched on open and rendered from sanitized
            // markdown (Text.MarkdownText renders, never executes).
            PlasmaComponents3.Label {
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.smallSpacing
                text: "What's new in " + (backend.updateNotes.version || page.update.available || "")
                font.bold: true
                font.pointSize: Kirigami.Theme.defaultFont.pointSize
            }

            QQC2.ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.gridUnit * 12
                clip: true

                QQC2.TextArea {
                    text: backend.updateNotes.body && backend.updateNotes.body.length > 0
                        ? backend.updateNotes.body
                        : "Loading release notes…"
                    textFormat: Text.MarkdownText
                    readOnly: true
                    selectByMouse: true
                    wrapMode: TextEdit.Wrap
                    color: Kirigami.Theme.textColor
                }
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: backend.updateNotes.source === "github" ? "Source: GitHub Releases" : (backend.updateNotes.source === "release-file" ? "Source: GitHub release notes" : (backend.updateNotes.source === "embedded" ? "Source: bundled changelog (offline)" : ""))
                opacity: 0.6
                elide: Text.ElideRight
                font.pointSize: Kirigami.Theme.smallFont.pointSize
                color: Kirigami.Theme.disabledTextColor
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            visible: page.update.outdated !== true

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                QQC2.Button {
                    text: "Check again"
                    icon.name: "view-refresh"
                    enabled: !backend.busy
                    onClicked: backend.refresh()
                }

                Item { Layout.fillWidth: true }
            }
        }

        Item { Layout.fillHeight: true }
    }
}
