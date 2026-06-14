import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kcmutils as KCM
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

KCM.SimpleKCM {
    id: page

    property string settingsQuery: ""

    leftPadding: Kirigami.Units.gridUnit * 2
    rightPadding: Kirigami.Units.gridUnit * 2
    topPadding: Kirigami.Units.gridUnit
    bottomPadding: Kirigami.Units.gridUnit

    SettingsBackend {
        id: backend
    }

    ColumnLayout {
        width: parent.width
        spacing: Kirigami.Units.smallSpacing

        QQC2.TextField {
            Layout.fillWidth: true
            text: page.settingsQuery
            placeholderText: "Search providers"
            selectByMouse: true
            leftPadding: Kirigami.Units.gridUnit * 1.6
            onTextChanged: page.settingsQuery = text

            Kirigami.Icon {
                anchors.left: parent.left
                anchors.leftMargin: Kirigami.Units.smallSpacing
                anchors.verticalCenter: parent.verticalCenter
                width: Kirigami.Units.iconSizes.small
                height: Kirigami.Units.iconSizes.small
                source: "search"
                opacity: 0.65
            }
        }

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            text: "Enable or disable providers. Use Manage accounts to add or remove named accounts; follow the connection instructions under each provider."
            wrapMode: Text.WordWrap
            opacity: 0.75
        }

        PlasmaComponents3.Label {
            visible: backend.filteredSettingsProviders(page.settingsQuery).length === 0
            Layout.fillWidth: true
            text: "No providers match “" + page.settingsQuery + "”."
            wrapMode: Text.WordWrap
            opacity: 0.7
        }

        Repeater {
            model: backend.filteredSettingsProviders(page.settingsQuery)

            delegate: ColumnLayout {
                id: settingsRow
                required property var modelData
                required property int index
                property bool manageOpen: false
                property string newName: ""
                property string newLabel: ""
                property var newFields: ({})
                property string newWsId: ""
                property string newWsName: ""
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        PlasmaComponents3.Label {
                            Layout.fillWidth: true
                            text: settingsRow.modelData.displayName || settingsRow.modelData.id
                            font.bold: true
                            elide: Text.ElideRight
                        }

                        PlasmaComponents3.Label {
                            visible: (settingsRow.modelData.accountText || "").length > 0
                            Layout.fillWidth: true
                            text: settingsRow.modelData.accountText || ""
                            opacity: 0.7
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                            elide: Text.ElideRight
                        }
                    }

                    QQC2.Switch {
                        checked: settingsRow.modelData.enabled === true
                        Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
                        onToggled: backend.setProviderEnabled(settingsRow.modelData.id, checked)
                    }
                }

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: settingsRow.modelData.connectHint || "Configure credentials, then refresh."
                    wrapMode: Text.WordWrap
                    opacity: 0.75
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    QQC2.ToolButton {
                        text: settingsRow.manageOpen ? "Hide accounts" : "Manage accounts"
                        icon.name: "user-group-properties"
                        display: QQC2.AbstractButton.TextBesideIcon
                        onClicked: {
                            settingsRow.manageOpen = !settingsRow.manageOpen
                            if (settingsRow.manageOpen) {
                                settingsRow.newName = ""
                                settingsRow.newLabel = ""
                                settingsRow.newFields = ({})
                            }
                        }
                    }
                    Item { Layout.fillWidth: true }
                }

                // ---- Account management (expandable) ----
                ColumnLayout {
                    visible: settingsRow.manageOpen
                    Layout.fillWidth: true
                    Layout.leftMargin: Kirigami.Units.largeSpacing
                    spacing: Kirigami.Units.smallSpacing

                    // Existing accounts with remove buttons.
                    Repeater {
                        model: settingsRow.modelData.accounts || []
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: Kirigami.Units.smallSpacing

                            PlasmaComponents3.Label {
                                Layout.fillWidth: true
                                text: (modelData.active === "true" ? "• " : "• (disabled) ") + (modelData.label || modelData.id)
                                elide: Text.ElideRight
                                opacity: 0.8
                                font.pointSize: Kirigami.Theme.smallFont.pointSize
                            }

                            QQC2.ToolButton {
                                visible: modelData.removable !== "false"
                                icon.name: "list-remove"
                                text: "Remove"
                                display: QQC2.AbstractButton.IconOnly
                                QQC2.ToolTip.visible: hovered
                                QQC2.ToolTip.text: "Remove account " + (modelData.label || modelData.id)
                                onClicked: backend.accountRemove(settingsRow.modelData.id, modelData.id)
                            }
                        }
                    }

                    // OAuth providers: the account must be created via terminal login.
                    PlasmaComponents3.Label {
                        visible: (settingsRow.modelData.setupHint || "").length > 0
                        Layout.fillWidth: true
                        text: settingsRow.modelData.setupHint
                        wrapMode: Text.WordWrap
                        opacity: 0.7
                        font.family: "monospace"
                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                    }

                    PlasmaComponents3.Label {
                        text: "Add account"
                        font.bold: true
                        opacity: 0.8
                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                    }

                    QQC2.TextField {
                        Layout.fillWidth: true
                        placeholderText: "Account name (e.g. work)"
                        selectByMouse: true
                        text: settingsRow.newName
                        onTextChanged: settingsRow.newName = text
                    }

                    QQC2.TextField {
                        Layout.fillWidth: true
                        placeholderText: "Label (optional)"
                        selectByMouse: true
                        text: settingsRow.newLabel
                        onTextChanged: settingsRow.newLabel = text
                    }

                    Repeater {
                        model: settingsRow.modelData.accountFields || []
                        delegate: QQC2.TextField {
                            required property var modelData
                            Layout.fillWidth: true
                            placeholderText: modelData.label + (modelData.placeholder ? " — " + modelData.placeholder : "")
                            echoMode: modelData.secret ? TextInput.Password : TextInput.Normal
                            selectByMouse: true
                            onTextChanged: {
                                var f = settingsRow.newFields
                                f[modelData.key] = text
                                settingsRow.newFields = f
                            }
                        }
                    }

                    PlasmaComponents3.Button {
                        text: "Add account"
                        icon.name: "list-add"
                        enabled: settingsRow.newName.trim().length > 0
                        onClicked: backend.accountSave(
                            settingsRow.modelData.id,
                            settingsRow.newName.trim(),
                            settingsRow.newLabel.trim(),
                            JSON.stringify(settingsRow.newFields))
                    }

                    // ---- opencode-go workspaces ----
                    ColumnLayout {
                        visible: settingsRow.modelData.authKind === "opencode"
                        Layout.fillWidth: true
                        Layout.topMargin: Kirigami.Units.smallSpacing
                        spacing: Kirigami.Units.smallSpacing

                        PlasmaComponents3.Label {
                            text: "Workspaces"
                            font.bold: true
                            opacity: 0.8
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                        }

                        Repeater {
                            model: settingsRow.modelData.workspaces || []
                            delegate: RowLayout {
                                required property var modelData
                                Layout.fillWidth: true
                                spacing: Kirigami.Units.smallSpacing

                                PlasmaComponents3.Label {
                                    Layout.fillWidth: true
                                    text: "• " + modelData.id + (modelData.name ? " — " + modelData.name : "")
                                    elide: Text.ElideRight
                                    opacity: 0.8
                                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                                }

                                QQC2.ToolButton {
                                    icon.name: "list-remove"
                                    text: "Remove"
                                    display: QQC2.AbstractButton.IconOnly
                                    QQC2.ToolTip.visible: hovered
                                    QQC2.ToolTip.text: "Remove workspace " + modelData.id
                                    onClicked: backend.workspaceRemove(modelData.id)
                                }
                            }
                        }

                        PlasmaComponents3.Label {
                            visible: !(settingsRow.modelData.workspaces && settingsRow.modelData.workspaces.length)
                            text: "No workspaces configured — auto-discovery is used."
                            opacity: 0.65
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                        }

                        QQC2.TextField {
                            Layout.fillWidth: true
                            placeholderText: "Workspace id (e.g. wrk_…)"
                            selectByMouse: true
                            text: settingsRow.newWsId
                            onTextChanged: settingsRow.newWsId = text
                        }

                        QQC2.TextField {
                            Layout.fillWidth: true
                            placeholderText: "Workspace name (optional)"
                            selectByMouse: true
                            text: settingsRow.newWsName
                            onTextChanged: settingsRow.newWsName = text
                        }

                        PlasmaComponents3.Button {
                            text: "Add workspace"
                            icon.name: "list-add"
                            enabled: settingsRow.newWsId.trim().length > 0
                            onClicked: backend.workspaceAdd(settingsRow.newWsId.trim(), settingsRow.newWsName.trim())
                        }
                    }
                }

                Rectangle {
                    visible: settingsRow.index < backend.filteredSettingsProviders(page.settingsQuery).length - 1
                    Layout.fillWidth: true
                    height: 1
                    color: Kirigami.Theme.disabledTextColor
                    opacity: 0.18
                }
            }
        }
    }
}
