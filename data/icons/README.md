# Custom SVG icons

Drop custom SVG icons into this folder and they will be bundled into the
editor's gresource at build time by `build.rs` (via
`relm4_icons_build::bundle_icons`).

## Rules

- File must be a single SVG (e.g. `drop.svg`, `water-drop.svg`).
- Use the filename (without the `.svg` extension) as the icon name.
- The name must also be listed in the icon name array in `build.rs` so
  that a matching Rust constant is generated in `icon_names.rs`.
- Custom icons keep their original colors unless their filename ends in
  `-symbolic` (then GTK will recolor them to match the current theme).

## Using a new icon

1. Add `my-icon.svg` to this folder.
2. Add `"my-icon"` to the icon name list in `build.rs`.
3. Rebuild. A constant `MY_ICON` (or `MY_ICON_SYMBOLIC` for symbolic
   variants) becomes available under `crate::icon_names`.
4. Reference it: `Image::from_icon_name(icon_names::MY_ICON)`.

## Replacing a shipped icon

To swap an existing shipped icon (e.g. `fog`) with a custom one, drop a
file with the same name here (e.g. `fog.svg`). The custom file takes
precedence over the bundled library icon during the build.
