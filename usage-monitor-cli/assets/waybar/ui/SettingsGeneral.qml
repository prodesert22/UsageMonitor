import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Plasma's configGeneral page.
//
// Ported difference: "Show bar text" is gone — Waybar renders the panel entry
// from the module's own `format`, so the widget cannot decide that. The popup
// placement (which Plasma handled by anchoring the dialog to the applet) is
// configured here instead.
QQC2.ScrollView {
    id: page

    // SettingsWindow instance.
    property var host: null
    readonly property var ui: page.host ? page.host.ui : null
    readonly property var settings: page.host ? page.host.settings : ({})

    readonly property var anchorIds: ["auto", "cursor", "top-right", "top-left",
                                      "bottom-right", "bottom-left", "top", "bottom", "center"]

    clip: true
    background: null

    ColumnLayout {
        width: page.availableWidth
        spacing: page.ui ? page.ui.largeSpacing : 8

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: page.ui ? page.ui.largeSpacing : 8
            spacing: page.ui ? page.ui.largeSpacing : 8

            ColumnLayout {
                spacing: 0

                QQC2.Label {
                    text: "Refresh every"
                    opacity: 0.8
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                }

                QQC2.SpinBox {
                    from: 10
                    to: 600
                    stepSize: 10
                    editable: true

                    // Editing replaces the binding, so the stored value is pushed
                    // back in whenever it changes (same hazard as in Plasma).
                    readonly property int storedValue: page.host
                        ? page.host.curr("refreshIntervalSeconds", page.settings.refreshIntervalSeconds || 30)
                        : 30
                    value: storedValue
                    onStoredValueChanged: if (value !== storedValue) value = storedValue
                    onValueModified: page.host.setPending("refreshIntervalSeconds", value)
                }
            }

            QQC2.Label {
                text: "seconds"
                Layout.alignment: Qt.AlignBottom
                opacity: 0.7
                bottomPadding: page.ui ? page.ui.smallSpacing : 4
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            ColumnLayout {
                spacing: 0

                QQC2.Label {
                    text: "Fetch providers at most every"
                    opacity: 0.8
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                }

                QQC2.SpinBox {
                    from: 0
                    to: 1800
                    stepSize: 30
                    editable: true

                    readonly property int storedValue: page.host
                        ? page.host.curr("minFetchIntervalSeconds", page.settings.minFetchIntervalSeconds || 180)
                        : 180
                    value: storedValue
                    onStoredValueChanged: if (value !== storedValue) value = storedValue
                    onValueModified: page.host.setPending("minFetchIntervalSeconds", value)
                }
            }

            QQC2.Label {
                text: "seconds"
                Layout.alignment: Qt.AlignBottom
                opacity: 0.7
                bottomPadding: page.ui ? page.ui.smallSpacing : 4
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            Item { Layout.fillWidth: true }
        }

        QQC2.Label {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            text: "The popup and the Waybar module share one fetched value for the second interval. "
                  + "Subscription endpoints (Claude, Codex) rate-limit hard below a few minutes, and a "
                  + "rate-limited fetch turns every provider into an error. The Refresh button always fetches."
            wrapMode: Text.WordWrap
            opacity: 0.65
            font.pointSize: page.ui ? page.ui.smallFontSize : 9
            color: page.ui ? page.ui.subtextColor : "#98989d"
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: 0

            QQC2.Label {
                text: "Show account email"
                opacity: 0.8
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            QQC2.Switch {
                id: showAccountEmailSwitch
                checked: page.host ? page.host.curr("showAccountEmail", page.settings.showAccountEmail !== false) : true
                onToggled: page.host.setPending("showAccountEmail", checked)

                QQC2.ToolTip.visible: showAccountEmailSwitch.hovered
                QQC2.ToolTip.text: "Show the account label below each provider in the usage view."
                QQC2.ToolTip.delay: 500
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: 0

            QQC2.Label {
                text: "Keep the popup open when it loses focus"
                opacity: 0.8
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            QQC2.Switch {
                checked: page.host ? page.host.curr("keepOpen", page.settings.keepOpen === true) : false
                onToggled: page.host.setPending("keepOpen", checked ? "true" : "false")
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: 0

            QQC2.Label {
                text: "Popup position"
                opacity: 0.8
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            QQC2.ComboBox {
                Layout.fillWidth: true
                model: ["Automatic (pointer, else top right)", "At the pointer", "Top right", "Top left",
                        "Bottom right", "Bottom left", "Top centre", "Bottom centre", "Screen centre"]
                currentIndex: {
                    var value = page.host ? String(page.host.curr("popupAnchor", page.settings.popupAnchor || "auto")) : "auto"
                    var index = page.anchorIds.indexOf(value)
                    return index < 0 ? 0 : index
                }
                onActivated: (index) => page.host.setPending("popupAnchor", page.anchorIds[index])
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: page.settings.session && page.settings.session.type === "wayland"
                      ? "This session is Wayland: a client cannot place its own window, so the compositor decides. "
                        + "Add a float/position rule for app-id \"usage-monitor-waybar\" (see docs/widgets/waybar.md); "
                        + "the setting still applies if you later run under X11."
                      : "Where the popup opens relative to the screen. Pass --anchor to the launcher to override it per click."
                wrapMode: Text.WordWrap
                opacity: 0.65
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.preferredHeight: 1
            color: page.ui ? page.ui.borderColor : "#48484a"
            opacity: 0.35
        }

        QQC2.Label {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            text: "Header text"
            opacity: 0.9
            font.pointSize: page.ui ? page.ui.smallFontSize : 9
            font.bold: true
            color: page.ui ? page.ui.textColor : "#f5f5f7"
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.bottomMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: page.ui ? page.ui.largeSpacing : 8

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                QQC2.Label {
                    text: "Pin to the popup header"
                    opacity: 0.8
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                }

                QQC2.ComboBox {
                    Layout.fillWidth: true
                    model: ["— none —"].concat((page.settings.pinnableProviders || []).map(function(p) {
                        return p.displayName || p.id
                    }))
                    currentIndex: {
                        var pinned = page.host ? page.host.curr("barProvider", page.settings.pinnedProvider || "") : ""
                        if (!pinned) return 0
                        var list = page.settings.pinnableProviders || []
                        for (var i = 0; i < list.length; i++) {
                            if (list[i].id === pinned) return i + 1
                        }
                        return 0
                    }
                    onActivated: (index) => {
                        if (index === 0) {
                            page.host.setPending("barProvider", "")
                        } else {
                            var list = page.settings.pinnableProviders || []
                            var picked = list[index - 1]
                            page.host.setPending("barProvider", picked.id || "")
                        }
                    }
                }

                QQC2.Label {
                    text: "Windows shown in the header text"
                    opacity: 0.8
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: page.ui ? page.ui.largeSpacing : 8

                    QQC2.CheckBox {
                        text: "Session (5h)"
                        checked: page.host ? page.host.curr("barSession", page.settings.barSession !== false) : true
                        onToggled: page.host.setPending("barSession", checked)
                    }

                    QQC2.CheckBox {
                        text: "Weekly"
                        checked: page.host ? page.host.curr("barWeekly", page.settings.barWeekly !== false) : true
                        onToggled: page.host.setPending("barWeekly", checked)
                    }

                    QQC2.CheckBox {
                        text: "Monthly"
                        checked: page.host ? page.host.curr("barMonthly", page.settings.barMonthly !== false) : true
                        onToggled: page.host.setPending("barMonthly", checked)
                    }
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    text: "The pinned provider drives the summary line at the top of the popup."
                    wrapMode: Text.WordWrap
                    opacity: 0.65
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.subtextColor : "#98989d"
                }
            }

            QQC2.Button {
                text: "Clear cache"
                Layout.alignment: Qt.AlignBottom
                onClicked: backend.cacheClear()

                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Clears the popup's last-good cache. Refresh after clearing."
                QQC2.ToolTip.delay: 500
            }
        }

        Item { Layout.fillHeight: true }
    }
}
