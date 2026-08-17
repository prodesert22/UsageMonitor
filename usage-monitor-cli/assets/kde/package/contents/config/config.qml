import QtQuick
import org.kde.plasma.configuration

ConfigModel {
    ConfigCategory {
        name: "General"
        icon: "configure"
        source: "configGeneral.qml"
    }
    ConfigCategory {
        name: "Providers"
        icon: "view-list-details"
        source: "configProviders.qml"
    }
    ConfigCategory {
        name: "Order"
        icon: "view-sort"
        source: "configOrder.qml"
    }
    ConfigCategory {
        name: "Theme"
        icon: "preferences-desktop-theme"
        source: "configTheme.qml"
    }
}
