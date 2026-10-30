# Session helper assets

`tray-icon.svg` is the notification-area icon: the "o" with its three nodes from the
wordmark (`modama-agent-gui/assets/modama-tint.svg`), white on the GUI's primary blue.
The other files are rendered from it and committed, so building needs no SVG renderer:

- `tray-icon.ico` (16, 20, 24, 32, 48, 256 px): the Windows tray icon
- `tray-icon-32.rgba`, `tray-icon-64.rgba`: raw 8-bit RGBA pixels for the Linux
  StatusNotifierItem

## Regenerating

ImageMagick's built-in SVG renderer drops stroke-only shapes (the nodes and links), so the
PNGs come from resvg 0.45, via a minimal program around
`resvg::render` that scales the SVG to a square `<size>` px pixmap and saves it as PNG
(`render-icon <svg> <size> <png>`). Then:

```sh
for s in 16 20 24 32 48 64 256; do render-icon tray-icon.svg $s tray-$s.png; done
magick tray-16.png tray-20.png tray-24.png tray-32.png tray-48.png tray-256.png tray-icon.ico
magick tray-32.png -depth 8 rgba:tray-icon-32.rgba
magick tray-64.png -depth 8 rgba:tray-icon-64.rgba
```
