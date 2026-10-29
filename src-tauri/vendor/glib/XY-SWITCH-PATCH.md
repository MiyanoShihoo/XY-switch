# XY Switch glib 0.18.5 compatibility backport

This is the checksum-verified crates.io glib 0.18.5 source, with the two-line upstream fix from gtk-rs/gtk-rs-core PR #1343 for RUSTSEC-2024-0429.

`VariantStrIter::impl_get` now uses a mutable local pointer and passes `&mut p` to the C variadic output parameter. Public APIs, feature flags and dependency requirements are unchanged. Upstream licenses and iterator tests are retained.

The package version stays 0.18.5 for compatibility with Tauri's GTK 0.18 dependency chain. Advisory databases may still report that upstream version; the saved patch provenance must be consulted. Linux optimized iterator tests still require a native GLib/GTK environment and are not represented as having run on Windows.
