import QtQuick
import org.kde.plasma.plasma5support as Plasma5Support

// Shared, non-visual plumbing for the config pages. Drives the usage-monitor-cli
// helper directly; changes apply immediately (settings live in the helper's
// state.json, not in KConfig).
Item {
    id: backend

    property var settings: ({
        "providers": [],
        "pinnableProviders": [],
        "pinnedProvider": "",
        "refreshIntervalSeconds": 30,
        "showBarText": true,
        "showAccountEmail": true,
        "showDecimals": true,
        "barSession": true,
        "barWeekly": true,
        "barMonthly": true,
        "providerOrder": "[]",
        "theme": ({ "mode": "plasma", "colors": ({}), "font": ({}), "metrics": ({}) }),
        "themeCatalog": ({ "builtin": [], "schemes": [] }),
        "themeState": ({}),
        "plasmoidVersion": "",
        "cliVersion": ""
    })
    property bool busy: false
    property string errorText: ""
    property string errorDetails: ""
    // Release notes for the Updates page (fetched on demand, sanitized by the
    // helper, rendered as markdown — never executed).
    property var updateNotes: ({"version": "", "url": "", "body": "", "source": ""})
    property string helperPath: localFilePath(Qt.resolvedUrl("../code/usage_monitor_kde.py"))
    property alias orderModel: providerOrderModel

    function localFilePath(url) {
        var text = String(url)
        if (text.indexOf("file://") === 0) {
            return decodeURIComponent(text.substring(7))
        }
        return text
    }

    function shellQuote(path) {
        return "'" + String(path).replace(/'/g, "'\\''") + "'"
    }

    function runHelper(command) {
        busy = true
        errorText = ""
        errorDetails = ""
        executor.connectSource("python3 " + shellQuote(helperPath) + " " + command)
    }

    function loadSettings() {
        runHelper("settings")
    }

    // Kept for delegates ported from the popup that call root.refresh().
    function refresh() {
        loadSettings()
    }

    function saveStateKey(key, value) {
        runHelper("set-state --key " + shellQuote(key) + " --value " + shellQuote(String(value)))
    }

    // Persist several state keys in one call (used by config pages on Apply/OK).
    //
    // The payload is built with JSON.stringify and quoted with shellQuote: theme
    // names, colours and font families are free text, and hand-rolled quoting
    // here used to let a single quote break out of the command line.
    function batchSetState(values) {
        var pairs = []
        for (var key in values) {
            pairs.push([String(key), String(values[key])])
        }
        if (pairs.length === 0)
            return
        runHelper("batch-set-state --json " + shellQuote(JSON.stringify(pairs)))
    }

    function setProviderEnabled(providerId, enabled) {
        runHelper("set-provider --provider " + shellQuote(providerId) + " --enabled " + (enabled ? "true" : "false"))
    }

    function accountSave(providerId, name, label, fieldsJson) {
        runHelper("account-save --provider " + shellQuote(providerId)
            + " --name " + shellQuote(name)
            + (label ? " --label " + shellQuote(label) : "")
            + " --json " + shellQuote(fieldsJson))
    }

    function accountRemove(providerId, name) {
        runHelper("account-remove --provider " + shellQuote(providerId) + " --name " + shellQuote(name))
    }

    function workspaceAdd(workspace, name) {
        runHelper("workspace-add --workspace " + shellQuote(workspace) + (name ? " --name " + shellQuote(name) : ""))
    }

    function workspaceRemove(workspace) {
        runHelper("workspace-remove --workspace " + shellQuote(workspace))
    }

    function cacheClear() {
        runHelper("cache-clear")
    }

    function updateShowChangelog() {
        var u = (backend.settings && backend.settings.update) || ({})
        if (!u.available || u.available === "unknown") {
            return
        }
        if (backend.updateNotes.version === u.available) {
            return
        }
        runHelper("update-changelog --version " + shellQuote(String(u.available)))
    }

    function updateApply() {
        runHelper("update-apply --target kde")
    }

    function updateDismiss() {
        var u = (backend.settings && backend.settings.update) || ({})
        if (u.available) {
            runHelper("update-dismiss --version " + shellQuote(String(u.available)))
        }
    }

    // Settings live in the helper's state.json, so saving one emits no KConfig
    // signal for the applet. Bumping this key (after the helper process has
    // finished writing) makes main.qml reload right away instead of showing the
    // old theme/settings until the next refresh tick.
    function notifyApplet() {
        if (typeof plasmoid !== "undefined" && plasmoid && plasmoid.configuration) {
            plasmoid.configuration.stateRevision = String(Date.now())
        }
    }

    function filteredSettingsProviders(query) {
        var providers = settings.providers || []
        var q = (query || "").trim().toLowerCase()
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

    function refreshProviderOrderModel() {
        providerOrderModel.clear()
        var orderArray = []
        try {
            var parsed = JSON.parse(settings.providerOrder || "[]")
            if (Array.isArray(parsed)) orderArray = parsed
        } catch (e) {}

        var allProviders = (settings.providers || []).filter(function(p) { return p.enabled !== false })
        var seen = {}
        var ordered = []
        for (var i = 0; i < orderArray.length; i++) {
            var oid = orderArray[i]
            for (var j = 0; j < allProviders.length; j++) {
                if (allProviders[j].id === oid) {
                    ordered.push({ id: oid, displayName: allProviders[j].displayName || oid })
                    seen[oid] = true
                    break
                }
            }
        }
        for (var k = 0; k < allProviders.length; k++) {
            var pid = allProviders[k].id || ""
            if (pid && !seen[pid]) {
                ordered.push({ id: pid, displayName: allProviders[k].displayName || pid })
            }
        }
        for (var m = 0; m < ordered.length; m++) {
            providerOrderModel.append({ providerId: ordered[m].id, displayName: ordered[m].displayName })
        }
    }

    function saveProviderOrder() {
        var ids = []
        for (var i = 0; i < providerOrderModel.count; i++) {
            ids.push(providerOrderModel.get(i).providerId)
        }
        saveStateKey("providerOrder", JSON.stringify(ids))
    }

    ListModel {
        id: providerOrderModel
    }

    Plasma5Support.DataSource {
        id: executor
        engine: "executable"
        connectedSources: []

        onNewData: function(sourceName, data) {
            executor.disconnectSource(sourceName)
            backend.busy = false

            if (data["exit code"] !== 0) {
                backend.errorText = "An error occurred."
                backend.errorDetails = data.stderr || data.stdout || "Usage Monitor helper failed"
                return
            }

            var isSettings = sourceName.indexOf(" settings") !== -1
            var isNotes = sourceName.indexOf(" update-changelog") !== -1
            var isManage = sourceName.indexOf(" set-provider ") !== -1
                || sourceName.indexOf(" set-state ") !== -1
                || sourceName.indexOf(" batch-set-state ") !== -1
                || sourceName.indexOf(" account-save ") !== -1
                || sourceName.indexOf(" account-remove ") !== -1
                || sourceName.indexOf(" workspace-add ") !== -1
                || sourceName.indexOf(" workspace-remove ") !== -1
                || sourceName.indexOf(" update-apply ") !== -1
                || sourceName.indexOf(" update-dismiss ") !== -1

            if (isManage) {
                backend.notifyApplet()
                backend.loadSettings()
                return
            }
            if (isNotes) {
                try {
                    var notes = JSON.parse(data.stdout)
                    if (notes && notes.version) {
                        backend.updateNotes = notes
                    }
                } catch (e) {
                    backend.errorText = "Invalid JSON from Usage Monitor helper"
                    backend.errorDetails = String(e)
                }
                return
            }
            if (isSettings) {
                try {
                    backend.settings = JSON.parse(data.stdout)
                    backend.refreshProviderOrderModel()
                    backend.errorText = ""
                    backend.errorDetails = ""
                } catch (e) {
                    backend.errorText = "Invalid JSON from Usage Monitor helper"
                    backend.errorDetails = String(e)
                }
            }
        }
    }

    Component.onCompleted: loadSettings()
}
