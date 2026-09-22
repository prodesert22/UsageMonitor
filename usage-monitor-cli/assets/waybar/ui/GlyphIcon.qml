import QtQuick

// Vector icons drawn on a Canvas.
//
// The Plasma widget used Kirigami.Icon with freedesktop icon names
// ("view-refresh", "configure", …). Waybar runs on setups that may have no icon
// theme installed at all — and on a bare Qt Quick app the theme lookup would
// silently render nothing — so the handful of icons the UI needs are drawn here
// from the palette's text colour instead. No icon theme, no font, no assets.
Canvas {
    id: glyph

    // One of: refresh, chart, pin, settings, close, add, remove, copy, clear,
    // undo, font, accounts, search, drag, check, update, info.
    property string name: "refresh"
    property color color: "#ffffff"
    property real thickness: Math.max(1.2, Math.min(width, height) / 11)

    implicitWidth: 16
    implicitHeight: 16
    antialiasing: true

    onNameChanged: requestPaint()
    onColorChanged: requestPaint()
    onWidthChanged: requestPaint()
    onHeightChanged: requestPaint()

    onPaint: {
        var ctx = getContext("2d")
        var w = width
        var h = height
        var s = Math.min(w, h)
        var cx = w / 2
        var cy = h / 2
        ctx.reset()
        ctx.clearRect(0, 0, w, h)
        ctx.strokeStyle = glyph.color
        ctx.fillStyle = glyph.color
        ctx.lineWidth = glyph.thickness
        ctx.lineCap = "round"
        ctx.lineJoin = "round"

        if (name === "refresh") {
            var r = s * 0.32
            ctx.beginPath()
            ctx.arc(cx, cy, r, Math.PI * 0.25, Math.PI * 1.75)
            ctx.stroke()
            // arrow head at the open end
            ctx.beginPath()
            ctx.moveTo(cx + r * 0.75, cy - r * 1.05)
            ctx.lineTo(cx + r * 0.72, cy - r * 0.15)
            ctx.lineTo(cx + r * 1.55, cy - r * 0.35)
            ctx.closePath()
            ctx.fill()
        } else if (name === "chart") {
            var base = cy + s * 0.3
            var bars = [0.28, 0.5, 0.38, 0.66]
            for (var i = 0; i < bars.length; i++) {
                var bw = s * 0.13
                var bx = cx - s * 0.33 + i * (bw + s * 0.06)
                ctx.fillRect(bx, base - s * bars[i], bw, s * bars[i])
            }
        } else if (name === "pin") {
            ctx.beginPath()
            ctx.moveTo(cx, cy + s * 0.38)
            ctx.lineTo(cx, cy + s * 0.05)
            ctx.stroke()
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.24, cy + s * 0.05)
            ctx.lineTo(cx + s * 0.24, cy + s * 0.05)
            ctx.lineTo(cx + s * 0.14, cy - s * 0.12)
            ctx.lineTo(cx + s * 0.18, cy - s * 0.34)
            ctx.lineTo(cx - s * 0.18, cy - s * 0.34)
            ctx.lineTo(cx - s * 0.14, cy - s * 0.12)
            ctx.closePath()
            ctx.fill()
        } else if (name === "settings") {
            var teeth = 8
            var outer = s * 0.36
            var inner = s * 0.26
            ctx.beginPath()
            for (var t = 0; t < teeth * 2; t++) {
                var angle = (Math.PI * t) / teeth
                var radius = (t % 2 === 0) ? outer : inner
                var px = cx + Math.cos(angle) * radius
                var py = cy + Math.sin(angle) * radius
                if (t === 0) {
                    ctx.moveTo(px, py)
                } else {
                    ctx.lineTo(px, py)
                }
            }
            ctx.closePath()
            ctx.stroke()
            ctx.beginPath()
            ctx.arc(cx, cy, s * 0.11, 0, Math.PI * 2)
            ctx.stroke()
        } else if (name === "close") {
            var d = s * 0.24
            ctx.beginPath()
            ctx.moveTo(cx - d, cy - d)
            ctx.lineTo(cx + d, cy + d)
            ctx.moveTo(cx + d, cy - d)
            ctx.lineTo(cx - d, cy + d)
            ctx.stroke()
        } else if (name === "add") {
            var a = s * 0.28
            ctx.beginPath()
            ctx.moveTo(cx - a, cy)
            ctx.lineTo(cx + a, cy)
            ctx.moveTo(cx, cy - a)
            ctx.lineTo(cx, cy + a)
            ctx.stroke()
        } else if (name === "remove") {
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.28, cy)
            ctx.lineTo(cx + s * 0.28, cy)
            ctx.stroke()
        } else if (name === "copy") {
            ctx.strokeRect(cx - s * 0.3, cy - s * 0.3, s * 0.42, s * 0.42)
            ctx.strokeRect(cx - s * 0.12, cy - s * 0.12, s * 0.42, s * 0.42)
        } else if (name === "clear") {
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.28, cy - s * 0.22)
            ctx.lineTo(cx + s * 0.28, cy - s * 0.22)
            ctx.stroke()
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.2, cy - s * 0.22)
            ctx.lineTo(cx - s * 0.14, cy + s * 0.3)
            ctx.lineTo(cx + s * 0.14, cy + s * 0.3)
            ctx.lineTo(cx + s * 0.2, cy - s * 0.22)
            ctx.stroke()
        } else if (name === "undo") {
            ctx.beginPath()
            ctx.arc(cx, cy + s * 0.05, s * 0.28, Math.PI, Math.PI * 2.15)
            ctx.stroke()
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.28, cy - s * 0.16)
            ctx.lineTo(cx - s * 0.28, cy + s * 0.06)
            ctx.lineTo(cx - s * 0.06, cy + s * 0.06)
            ctx.stroke()
        } else if (name === "font") {
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.28, cy + s * 0.3)
            ctx.lineTo(cx, cy - s * 0.3)
            ctx.lineTo(cx + s * 0.28, cy + s * 0.3)
            ctx.moveTo(cx - s * 0.16, cy + s * 0.08)
            ctx.lineTo(cx + s * 0.16, cy + s * 0.08)
            ctx.stroke()
        } else if (name === "accounts") {
            ctx.beginPath()
            ctx.arc(cx, cy - s * 0.14, s * 0.16, 0, Math.PI * 2)
            ctx.stroke()
            ctx.beginPath()
            ctx.arc(cx, cy + s * 0.42, s * 0.3, Math.PI * 1.15, Math.PI * 1.85)
            ctx.stroke()
        } else if (name === "search") {
            ctx.beginPath()
            ctx.arc(cx - s * 0.06, cy - s * 0.06, s * 0.22, 0, Math.PI * 2)
            ctx.stroke()
            ctx.beginPath()
            ctx.moveTo(cx + s * 0.1, cy + s * 0.1)
            ctx.lineTo(cx + s * 0.3, cy + s * 0.3)
            ctx.stroke()
        } else if (name === "drag") {
            for (var row = -1; row <= 1; row++) {
                for (var col = -1; col <= 1; col += 2) {
                    ctx.beginPath()
                    ctx.arc(cx + col * s * 0.14, cy + row * s * 0.2, glyph.thickness * 0.8, 0, Math.PI * 2)
                    ctx.fill()
                }
            }
        } else if (name === "check") {
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.26, cy)
            ctx.lineTo(cx - s * 0.06, cy + s * 0.2)
            ctx.lineTo(cx + s * 0.28, cy - s * 0.22)
            ctx.stroke()
        } else if (name === "update") {
            // Down arrow into a tray: a newer version is ready to download.
            ctx.beginPath()
            ctx.moveTo(cx, cy - s * 0.32)
            ctx.lineTo(cx, cy + s * 0.12)
            ctx.stroke()
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.18, cy - s * 0.06)
            ctx.lineTo(cx, cy + s * 0.12)
            ctx.lineTo(cx + s * 0.18, cy - s * 0.06)
            ctx.stroke()
            ctx.beginPath()
            ctx.moveTo(cx - s * 0.3, cy + s * 0.3)
            ctx.lineTo(cx + s * 0.3, cy + s * 0.3)
            ctx.stroke()
        } else if (name === "info") {
            ctx.beginPath()
            ctx.arc(cx, cy, s * 0.3, 0, Math.PI * 2)
            ctx.stroke()
            ctx.beginPath()
            ctx.arc(cx, cy - s * 0.12, glyph.thickness * 0.7, 0, Math.PI * 2)
            ctx.fill()
            ctx.beginPath()
            ctx.moveTo(cx, cy - s * 0.0)
            ctx.lineTo(cx, cy + s * 0.2)
            ctx.stroke()
        }
    }
}
