import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

// Self-update banner: shown when the installed widget is older than the CLI
// binary. "What's new" fetches the release notes (GitHub Releases, cached),
// "Update now" reinstalls the widget the same way `widget install` does.
ColumnLayout {
    id: banner

    readonly property var update: (root.settings && root.settings.update) || ({})

    visible: update.outdated === true && update.dismissed !== true
    spacing: Kirigami.Units.smallSpacing

    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: bannerContent.implicitHeight + Kirigami.Units.smallSpacing * 2
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
            anchors.margins: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Icon {
                    source: "system-software-update"
                    Layout.preferredWidth: Kirigami.Units.iconSizes.small
                    Layout.preferredHeight: Kirigami.Units.iconSizes.small
                    Layout.alignment: Qt.AlignVCenter
                }

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignVCenter
                    text: "Update " + (banner.update.available || "?") + " available (installed " + (banner.update.installed || "?") + ")"
                    elide: Text.ElideRight
                    maximumLineCount: 1
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.fontSize
                    color: root.ui.textColor
                }

                PlasmaComponents3.ToolButton {
                    icon.name: "dialog-close"
                    Layout.alignment: Qt.AlignVCenter
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: "Hide this update notice"
                    onClicked: root.updateDismiss()
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Item { Layout.fillWidth: true }

                PlasmaComponents3.Button {
                    text: "What's new"
                    icon.name: "help-about"
                    Layout.alignment: Qt.AlignVCenter
                    onClicked: root.updateShowChangelog()
                }

                PlasmaComponents3.Button {
                    text: root.updateApplying ? "Updating…" : "Update now"
                    icon.name: "system-software-update"
                    enabled: !root.updateApplying && !root.busy
                    Layout.alignment: Qt.AlignVCenter
                    onClicked: root.updateApply()
                }
            }
        }
    }

    PlasmaComponents3.Label {
        visible: root.updateApplying || root.updateDone.length > 0
        Layout.fillWidth: true
        text: root.updateApplying ? "Updating widget…" : root.updateDone
        wrapMode: Text.WordWrap
        opacity: 0.8
        font.family: root.ui.fontFamily
        font.pointSize: root.ui.smallFontSize
        color: root.ui.subtextColor
    }

    // Release notes, fetched on demand by `update-changelog --version` and
    // shown inline (a Dialog would need an Overlay that the Plasma popup
    // does not provide).
    ColumnLayout {
        visible: root.updateNotesOpen
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            text: "What's new in " + (root.updateNotes.version || "")
            font.bold: true
            font.family: root.ui.fontFamily
            font.pointSize: root.ui.fontSize
            color: root.ui.textColor
        }

        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 10
            clip: true

            QQC2.TextArea {
                    text: root.updateNotes.body && root.updateNotes.body.length > 0
                        ? root.updateNotes.body
                        : "No notes available offline. See the release page instead."
                    textFormat: Text.MarkdownText
                    readOnly: true
                    selectByMouse: true
                    wrapMode: TextEdit.Wrap
                    color: root.ui.textColor
                }
        }

            RowLayout {
                Layout.fillWidth: true
                // Action first: at the right edge the button would sit in the
                // scrollbar column of the notes view above.
                PlasmaComponents3.Button {
                    text: "Open release page"
                    icon.name: "internet-web-browser"
                    visible: (root.updateNotes.url || "").length > 0
                    Layout.alignment: Qt.AlignVCenter
                    onClicked: Qt.openUrlExternally(root.updateNotes.url)
                }
                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: root.updateNotes.source === "github" ? "Source: GitHub Releases" : (root.updateNotes.source === "release-file" ? "Source: GitHub release notes" : (root.updateNotes.source === "embedded" ? "Source: bundled changelog (offline)" : ""))
                    opacity: 0.6
                    elide: Text.ElideRight
                    font.pointSize: root.ui.smallFontSize
                    color: root.ui.subtextColor
                }
            }
    }
}
