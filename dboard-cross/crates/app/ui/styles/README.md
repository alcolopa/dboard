# dboard control style

This style adapts Slint 1.18.1's Fluent widgets so standard controls use
`../theme.slint` just like the rest of the app. `dboard/styling.slint` maps
control roles to theme tokens; keep color literals in `../theme.slint`.

Source: Slint's `internal/compiler/widgets/fluent` and shared `common` widgets,
https://github.com/slint-ui/slint/tree/v1.18.1/internal/compiler/widgets .
Original copyright and SPDX license notices are retained in each source file.
SVG files are monochrome icon masks, tinted by the theme at runtime.

Only reachable widget modules are included. LineEdit's interface forwarding is
expanded into stable property/callback aliases; internal-only property annotations
are removed. Internal menu types are enabled during Slint compilation. The app selects this style in `build.rs` so colors behave consistently
on macOS, Windows, and Linux instead of following the operating system accent.
When updating Slint, review these adaptations and run the UI tests.
