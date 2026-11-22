# Third-party notices

Rexy is an unofficial client for Rex. It is not affiliated with Superlogical.

The Rex terminal metadata parser and parts of color/cell/cursor conversion in `src/terminal.rs` are adapted from [rex-rune](https://github.com/shuv1337/rex-rune), snapshot `e914b1987681d941ddde537b37dd07acbbb146f2`. The names each program icon goes by, in `src/program.rs`, are adapted from its snapshot `db0f86c`. The transport and layout implementation also used that project's interoperability behavior and [rexrs](https://github.com/a10y/rexrs) as protocol references. No Rune UI code is included.

## Adapted rex-rune code

MIT License

Copyright (c) 2026 shuv

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Fonts

`assets/fonts/` holds [Inter](https://github.com/rsms/inter) 4.1 and [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) 2.304, upright and italic, unmodified variable fonts under the SIL Open Font License 1.1. Each licence is beside its font.

## Program icons

Tabs show the tiles of Superlogical's Rex icon set, as converted by rex-rune. They are Superlogical's artwork of other owners' logos, with no published terms, so they are not kept in this repository: `scripts/icons` fetches them into `assets/icons/`, and a build includes whichever are there. See `assets/icons/NOTICE.md` before passing such a build on.

On Windows, a build may also carry Rex's own app icon, from Rex.app, in place of Rexy's drawing. It is Superlogical's artwork too and is not kept here: see `assets/windows/NOTICE.md`.

## Libraries

libghostty is used through the `libghostty-vt` Rust bindings of [libghostty-rs](https://github.com/Uzaaft/libghostty-rs), at a commit named in `Cargo.toml`: no release of them can yet restore a terminal snapshot. egui/eframe supplies the desktop interface, and resvg draws the icons. These and the remaining Cargo dependencies retain their respective upstream licenses; exact versions and sources are recorded in `Cargo.lock`.
