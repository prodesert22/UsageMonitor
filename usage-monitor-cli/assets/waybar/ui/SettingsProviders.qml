import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Plasma's configProviders page: enable/disable providers and manage named
// accounts. These act on the CLI immediately (they are not state.json keys),
// so they are not part of the pending/Apply cycle — same as in Plasma.
QQC2.ScrollView {
    id: page

    property var host: null
    readonly property var ui: page.host ? page.host.ui : null
    readonly property var settings: page.host ? page.host.settings : ({})

    property string query: ""

    clip: true
    background: null

    function filteredProviders() {
        var providers = page.settings.providers || []
        var q = (page.query || "").trim().toLowerCase()
        if (!q) {
            return providers
        }
        return providers.filter(function(provider) {
            var haystack = [
                provider.id || "",
                provider.displayName || "",
                provider.accountText || "",
                provider.connectHint || ""
            ].join(" ").toLowerCase()
            return haystack.indexOf(q) !== -1
        })
    }

    ColumnLayout {
        width: page.availableWidth
        spacing: page.ui ? page.ui.smallSpacing : 4

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: page.ui ? page.ui.largeSpacing : 8
            spacing: page.ui ? page.ui.smallSpacing : 4

            GlyphIcon {
                name: "search"
                color: page.ui ? page.ui.subtextColor : "#98989d"
                implicitWidth: page.ui ? page.ui.iconSize : 16
                implicitHeight: page.ui ? page.ui.iconSize : 16
                Layout.alignment: Qt.AlignVCenter
            }

            QQC2.TextField {
                Layout.fillWidth: true
                text: page.query
                placeholderText: "Search providers"
                selectByMouse: true
                onTextChanged: page.query = text
            }
        }

        QQC2.Label {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            text: "Enable or disable providers. Use Manage accounts to add or remove named accounts; follow the connection instructions under each provider."
            wrapMode: Text.WordWrap
            opacity: 0.75
            color: page.ui ? page.ui.textColor : "#f5f5f7"
        }

        QQC2.Label {
            visible: page.filteredProviders().length === 0
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            text: page.query.length > 0
                  ? "No providers match “" + page.query + "”."
                  : "No providers reported by the CLI. Check `usage-monitor-cli list`."
            wrapMode: Text.WordWrap
            opacity: 0.7
            color: page.ui ? page.ui.subtextColor : "#98989d"
        }

        Repeater {
            model: page.filteredProviders()

            delegate: ColumnLayout {
                id: providerRow
                required property var modelData
                required property int index
                property bool manageOpen: false
                property string newName: ""
                property string newLabel: ""
                property var newFields: ({})
                Layout.fillWidth: true
                Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
                Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
                spacing: page.ui ? page.ui.smallSpacing : 4

                RowLayout {
                    Layout.fillWidth: true
                    spacing: page.ui ? page.ui.smallSpacing : 4

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        QQC2.Label {
                            Layout.fillWidth: true
                            text: providerRow.modelData.displayName || providerRow.modelData.id
                            font.bold: true
                            elide: Text.ElideRight
                            color: page.ui ? page.ui.textColor : "#f5f5f7"
                        }

                        QQC2.Label {
                            visible: (providerRow.modelData.accountText || "").length > 0
                            Layout.fillWidth: true
                            text: providerRow.modelData.accountText || ""
                            opacity: 0.7
                            font.pointSize: page.ui ? page.ui.smallFontSize : 9
                            elide: Text.ElideRight
                            color: page.ui ? page.ui.subtextColor : "#98989d"
                        }
                    }

                    QQC2.Switch {
                        checked: providerRow.modelData.enabled === true
                        Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
                        onToggled: backend.setProviderEnabled(providerRow.modelData.id, checked)
                    }
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    text: providerRow.modelData.connectHint || "Configure credentials, then refresh."
                    wrapMode: Text.WordWrap
                    opacity: 0.75
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.subtextColor : "#98989d"
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: page.ui ? page.ui.smallSpacing : 4

                    QQC2.Button {
                        text: providerRow.manageOpen ? "Hide accounts" : "Manage accounts"
                        onClicked: {
                            providerRow.manageOpen = !providerRow.manageOpen
                            if (providerRow.manageOpen) {
                                providerRow.newName = ""
                                providerRow.newLabel = ""
                                providerRow.newFields = ({})
                            }
                        }
                    }
                    Item { Layout.fillWidth: true }
                }

                // ---- Account management (expandable) ----
                ColumnLayout {
                    visible: providerRow.manageOpen
                    Layout.fillWidth: true
                    Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
                    spacing: page.ui ? page.ui.smallSpacing : 4

                    Repeater {
                        model: providerRow.modelData.accounts || []
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: page.ui ? page.ui.smallSpacing : 4

                            QQC2.Label {
                                Layout.fillWidth: true
                                text: (modelData.active === "true" ? "• " : "• (disabled) ")
                                      + (modelData.label || modelData.id)
                                elide: Text.ElideRight
                                opacity: 0.8
                                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                                color: page.ui ? page.ui.textColor : "#f5f5f7"
                            }

                            ThemedToolButton {
                                visible: modelData.removable !== "false"
                                ui: page.ui
                                iconName: "remove"
                                text: "Remove"
                                tooltipText: "Remove account " + (modelData.label || modelData.id)
                                onClicked: backend.accountRemove(providerRow.modelData.id, modelData.id)
                            }
                        }
                    }

                    // OAuth providers: the account must be created via terminal login.
                    QQC2.Label {
                        visible: (providerRow.modelData.setupHint || "").length > 0
                        Layout.fillWidth: true
                        text: providerRow.modelData.setupHint || ""
                        wrapMode: Text.WordWrap
                        opacity: 0.7
                        font.family: "monospace"
                        font.pointSize: page.ui ? page.ui.smallFontSize : 9
                        color: page.ui ? page.ui.subtextColor : "#98989d"
                    }

                    QQC2.Label {
                        text: "Add account"
                        font.bold: true
                        opacity: 0.8
                        font.pointSize: page.ui ? page.ui.smallFontSize : 9
                        color: page.ui ? page.ui.textColor : "#f5f5f7"
                    }

                    QQC2.TextField {
                        Layout.fillWidth: true
                        placeholderText: "Account name (e.g. work)"
                        selectByMouse: true
                        text: providerRow.newName
                        onTextChanged: providerRow.newName = text
                    }

                    QQC2.TextField {
                        Layout.fillWidth: true
                        placeholderText: "Label (optional)"
                        selectByMouse: true
                        text: providerRow.newLabel
                        onTextChanged: providerRow.newLabel = text
                    }

                    Repeater {
                        model: providerRow.modelData.accountFields || []
                        delegate: QQC2.TextField {
                            required property var modelData
                            Layout.fillWidth: true
                            placeholderText: modelData.label + (modelData.placeholder ? " — " + modelData.placeholder : "")
                            echoMode: modelData.secret ? TextInput.Password : TextInput.Normal
                            selectByMouse: true
                            onTextChanged: {
                                var fields = providerRow.newFields
                                fields[modelData.key] = text
                                providerRow.newFields = fields
                            }
                        }
                    }

                    QQC2.Button {
                        text: "Add account"
                        enabled: providerRow.newName.trim().length > 0
                        onClicked: backend.accountSave(
                            providerRow.modelData.id,
                            providerRow.newName.trim(),
                            providerRow.newLabel.trim(),
                            JSON.stringify(providerRow.newFields))
                    }
                }

                Rectangle {
                    visible: providerRow.index < page.filteredProviders().length - 1
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    color: page.ui ? page.ui.borderColor : "#48484a"
                    opacity: 0.35
                }
            }
        }

        Item { Layout.preferredHeight: page.ui ? page.ui.largeSpacing : 8 }
    }
}
